// Command runner is the generic cross-language conformance runner for
// generated *Go* packages.
//
// It is driven by tests/json_schema_conformance_manifest.rs through a plan file
// (protocol in tests/toolchain/mod.rs). Models reach it through registry.go,
// which the Rust driver generates next to this file because Go has no way to
// look a type up by name at run time; everything past that point is reflection,
// so a new conformance case needs no runner change.
package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"reflect"
	"regexp"
	"strconv"
	"strings"

	"go.temporal.io/sdk/temporal"
)

type mutation struct {
	Path               string         `json:"path"`
	SetInteger         *string        `json:"set_integer"`
	SetNumber          *string        `json:"set_number"`
	SetString          *string        `json:"set_string"`
	SetNull            *bool          `json:"set_null"`
	DuplicateElement   *int           `json:"duplicate_element"`
	RemoveArrayElement *int           `json:"remove_array_element"`
	PutMapEntry        *mapEntry      `json:"put_map_entry"`
	RemoveMapEntry     *string        `json:"remove_map_entry"`
	SetAbsent          *bool          `json:"set_absent"`
	SetBytes           []byte         `json:"set_bytes"`
	SetDuration        *durationValue `json:"set_duration"`
}

type mapEntry struct {
	Key   string          `json:"key"`
	Value json.RawMessage `json:"value"`
}

type durationValue struct {
	Seconds     int64 `json:"seconds"`
	Nanoseconds int64 `json:"nanoseconds"`
}

type probe struct {
	ID        string     `json:"id"`
	Kind      string     `json:"kind"`
	Wire      string     `json:"wire"`
	Mutations []mutation `json:"mutations"`
}

type conformanceCase struct {
	ID     string  `json:"id"`
	Dir    string  `json:"dir"`
	Model  string  `json:"model"`
	Probes []probe `json:"probes"`
}

type plan struct {
	Cases []conformanceCase `json:"cases"`
}

type violation struct {
	Path   string `json:"path"`
	Reason string `json:"reason"`
}

type verdict struct {
	Outcome    string      `json:"outcome"`
	Violations []violation `json:"violations,omitempty"`
	Wire       *string     `json:"wire,omitempty"`
	Note       string      `json:"note,omitempty"`
	Message    string      `json:"message,omitempty"`
}

func main() {
	if err := run(os.Args[1], os.Args[2]); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(planPath, resultPath string) error {
	raw, err := os.ReadFile(planPath)
	if err != nil {
		return err
	}
	var loaded plan
	if err := json.Unmarshal(raw, &loaded); err != nil {
		return err
	}
	results := map[string]map[string]verdict{}
	for _, testCase := range loaded.Cases {
		probes := map[string]verdict{}
		results[testCase.ID] = probes
		modelType, ok := registry[testCase.ID]
		if !ok {
			for _, p := range testCase.Probes {
				probes[p.ID] = verdict{Outcome: "error", Message: "case missing from the generated registry"}
			}
			continue
		}
		for _, p := range testCase.Probes {
			probes[p.ID] = runProbe(modelType, p)
		}
	}
	encoded, err := json.MarshalIndent(results, "", " ")
	if err != nil {
		return err
	}
	return os.WriteFile(resultPath, encoded, 0o644)
}

func runProbe(modelType reflect.Type, p probe) verdict {
	pointer := reflect.New(modelType)
	if err := json.Unmarshal([]byte(p.Wire), pointer.Interface()); err != nil {
		if found, ok := violationsOf(err); ok {
			return verdict{Outcome: "parse_rejected", Violations: found}
		}
		return verdict{Outcome: "error", Message: err.Error()}
	}
	if p.Kind == "parse" {
		return verdict{Outcome: "accepted"}
	}
	for _, m := range p.Mutations {
		if err := applyMutation(pointer.Elem(), m); err != nil {
			return verdict{Outcome: "error", Message: "mutation failed: " + err.Error()}
		}
	}
	encoded, err := json.Marshal(pointer.Elem().Interface())
	if err != nil {
		if found, ok := violationsOf(err); ok {
			return verdict{Outcome: "serialize_rejected", Violations: found}
		}
		// encoding/json refused the value the model was willing to emit — for
		// example a non-finite float. The model accepted it; the wire did not.
		return verdict{Outcome: "accepted", Note: "output is not JSON: " + err.Error()}
	}
	wire := string(encoded)
	return verdict{Outcome: "accepted", Wire: &wire}
}

// violationsOf reports a payload-validation ApplicationError's first detail.
// The generated Violation type is package-local to each conformance case, so
// the runner first casts the detail to any and then reads its common shape.
// Details performs a cheap local assignment for newly-created errors; it does
// not serialize the violations.
func violationsOf(err error) ([]violation, bool) {
	var applicationError *temporal.ApplicationError
	if !errors.As(err, &applicationError) || applicationError.Type() != "PayloadValidationError" {
		return nil, false
	}
	var detail any
	if err := applicationError.Details(&detail); err != nil {
		return nil, false
	}
	value := reflect.ValueOf(detail)
	if value.Kind() != reflect.Slice {
		return nil, false
	}
	found := make([]violation, 0, value.Len())
	for index := 0; index < value.Len(); index++ {
		element := value.Index(index)
		found = append(found, violation{
			Path:   element.FieldByName("Path").String(),
			Reason: element.FieldByName("Reason").String(),
		})
	}
	return found, true
}

// step is one component of a mutation path: a named member, or an array index.
type step struct {
	name  string
	index int
}

var segmentPattern = regexp.MustCompile(`^([A-Za-z0-9]+)((?:\[\d+\])*)$`)
var indexPattern = regexp.MustCompile(`\[(\d+)\]`)

// stepsOf turns `a.b[0][1]` into field a, field b, index 0, index 1.
func stepsOf(path string) ([]step, error) {
	var steps []step
	for _, segment := range strings.Split(path, ".") {
		match := segmentPattern.FindStringSubmatch(segment)
		if match == nil {
			return nil, fmt.Errorf("unparsable path segment %q", segment)
		}
		steps = append(steps, step{name: match[1], index: -1})
		for _, found := range indexPattern.FindAllStringSubmatch(match[2], -1) {
			at, err := strconv.Atoi(found[1])
			if err != nil {
				return nil, err
			}
			steps = append(steps, step{index: at})
		}
	}
	return steps, nil
}

func read(owner reflect.Value, s step) (reflect.Value, error) {
	for owner.Kind() == reflect.Pointer || owner.Kind() == reflect.Interface {
		owner = owner.Elem()
	}
	if s.name == "" {
		if owner.Kind() != reflect.Slice && owner.Kind() != reflect.Array {
			return reflect.Value{}, fmt.Errorf("index into a %s", owner.Kind())
		}
		return owner.Index(s.index), nil
	}
	return fieldByJSONName(owner, s.name)
}

func applyMutation(model reflect.Value, m mutation) error {
	steps, err := stepsOf(m.Path)
	if err != nil {
		return err
	}
	owner := model
	for _, s := range steps[:len(steps)-1] {
		owner, err = read(owner, s)
		if err != nil {
			return err
		}
	}
	target, err := read(owner, steps[len(steps)-1])
	if err != nil {
		return err
	}
	if m.DuplicateElement != nil {
		if target.Kind() != reflect.Slice {
			return fmt.Errorf("%s is not a slice", m.Path)
		}
		target.Set(reflect.Append(target, target.Index(*m.DuplicateElement)))
		return nil
	}
	if m.RemoveArrayElement != nil {
		if target.Kind() != reflect.Slice {
			return fmt.Errorf("%s is not a slice", m.Path)
		}
		at := *m.RemoveArrayElement
		if at < 0 || at >= target.Len() {
			return fmt.Errorf("array index %d is out of range for %s", at, m.Path)
		}
		target.Set(reflect.AppendSlice(target.Slice(0, at), target.Slice(at+1, target.Len())))
		return nil
	}
	if m.PutMapEntry != nil {
		target = typedMap(target)
		if target.Kind() != reflect.Map {
			return fmt.Errorf("%s is not a map", m.Path)
		}
		value := reflect.New(target.Type().Elem())
		if err := json.Unmarshal(m.PutMapEntry.Value, value.Interface()); err != nil {
			return err
		}
		target.SetMapIndex(reflect.ValueOf(m.PutMapEntry.Key).Convert(target.Type().Key()), value.Elem())
		return nil
	}
	if m.RemoveMapEntry != nil {
		target = typedMap(target)
		if target.Kind() != reflect.Map {
			return fmt.Errorf("%s is not a map", m.Path)
		}
		target.SetMapIndex(reflect.ValueOf(*m.RemoveMapEntry).Convert(target.Type().Key()), reflect.Value{})
		return nil
	}
	return assign(target, m)
}

// Typed additional properties are represented directly as a map in some
// positions and as a generated object with an AdditionalProperties field in
// others. The mutation protocol intentionally hides that target detail.
func typedMap(target reflect.Value) reflect.Value {
	for target.Kind() == reflect.Pointer || target.Kind() == reflect.Interface {
		target = target.Elem()
	}
	if target.Kind() == reflect.Struct {
		if field := target.FieldByName("AdditionalProperties"); field.IsValid() {
			return field
		}
	}
	return target
}

func assign(target reflect.Value, m mutation) error {
	if m.SetAbsent != nil {
		target.Set(reflect.Zero(target.Type()))
		return nil
	}
	if m.SetNull != nil {
		if target.Kind() != reflect.Pointer && target.Kind() != reflect.Slice && target.Kind() != reflect.Map {
			return fmt.Errorf("%s is not nullable", m.Path)
		}
		target.Set(reflect.Zero(target.Type()))
		return nil
	}
	if target.Kind() == reflect.Pointer {
		if target.IsNil() {
			target.Set(reflect.New(target.Type().Elem()))
		}
		target = target.Elem()
	}
	switch {
	case m.SetBytes != nil:
		if target.Kind() != reflect.Slice || target.Type().Elem().Kind() != reflect.Uint8 {
			return fmt.Errorf("%s is not native bytes", m.Path)
		}
		target.SetBytes(m.SetBytes)
	case m.SetDuration != nil:
		if target.Kind() != reflect.Int64 {
			return fmt.Errorf("%s is not a native duration", m.Path)
		}
		target.SetInt(m.SetDuration.Seconds*1_000_000_000 + m.SetDuration.Nanoseconds)
	case m.SetInteger != nil:
		parsed, err := strconv.ParseInt(*m.SetInteger, 10, 64)
		if err != nil {
			return err
		}
		if target.Kind() == reflect.Float64 {
			target.SetFloat(float64(parsed))
			return nil
		}
		target.SetInt(parsed)
	case m.SetNumber != nil:
		target.SetFloat(numberOf(*m.SetNumber))
	case m.SetString != nil:
		target.SetString(*m.SetString)
	default:
		return fmt.Errorf("unknown mutation for %s", m.Path)
	}
	return nil
}

func numberOf(spec string) float64 {
	switch spec {
	case "nan":
		return math.NaN()
	case "inf":
		return math.Inf(1)
	case "-inf":
		return math.Inf(-1)
	}
	parsed, err := strconv.ParseFloat(spec, 64)
	if err != nil {
		panic(err)
	}
	return parsed
}

// fieldByJSONName finds a struct field by its `json:"..."` tag rather than by a
// derived Go identifier, so the runner never has to reimplement the emitter's
// naming rules.
func fieldByJSONName(owner reflect.Value, name string) (reflect.Value, error) {
	if owner.Kind() != reflect.Struct {
		return reflect.Value{}, fmt.Errorf("%s is not a struct", owner.Kind())
	}
	structType := owner.Type()
	for index := 0; index < structType.NumField(); index++ {
		tag := structType.Field(index).Tag.Get("json")
		if tag == "" {
			continue
		}
		if strings.Split(tag, ",")[0] == name {
			return owner.Field(index), nil
		}
	}
	return reflect.Value{}, fmt.Errorf("no field tagged %q on %s", name, structType.Name())
}
