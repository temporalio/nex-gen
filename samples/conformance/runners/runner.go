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
)

type mutation struct {
	Path             string  `json:"path"`
	SetInteger       *string `json:"set_integer"`
	SetNumber        *string `json:"set_number"`
	SetString        *string `json:"set_string"`
	SetNull          *bool   `json:"set_null"`
	DuplicateElement *int    `json:"duplicate_element"`
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

// violationsOf reports the generated *ValidationError's violations. The type is
// package-local to every generated module, so it is matched structurally.
func violationsOf(err error) ([]violation, bool) {
	for current := err; current != nil; current = errors.Unwrap(current) {
		value := reflect.ValueOf(current)
		if value.Kind() == reflect.Pointer {
			if value.IsNil() {
				continue
			}
			value = value.Elem()
		}
		if value.Kind() != reflect.Struct || value.Type().Name() != "ValidationError" {
			continue
		}
		field := value.FieldByName("Violations")
		if !field.IsValid() || field.Kind() != reflect.Slice {
			continue
		}
		found := make([]violation, 0, field.Len())
		for index := 0; index < field.Len(); index++ {
			element := field.Index(index)
			found = append(found, violation{
				Path:   element.FieldByName("Path").String(),
				Reason: element.FieldByName("Reason").String(),
			})
		}
		return found, true
	}
	return nil, false
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
	return assign(target, m)
}

func assign(target reflect.Value, m mutation) error {
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
