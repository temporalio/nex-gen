package tests

import "reflect"

type nexusCall struct {
	Operation string
	Input     any
}

func operationNames(calls []nexusCall) []string {
	names := make([]string, len(calls))
	for i, call := range calls {
		names[i] = call.Operation
	}
	return names
}

func stringField(value any, name string) string {
	reflected := reflect.ValueOf(value)
	if reflected.Kind() == reflect.Pointer {
		reflected = reflected.Elem()
	}
	if reflected.Kind() == reflect.Struct {
		field := reflected.FieldByName(name)
		if field.IsValid() && field.Kind() == reflect.String {
			return field.String()
		}
	}
	if reflected.Kind() == reflect.Map && reflected.Type().Key().Kind() == reflect.String {
		mapValue := reflected.MapIndex(reflect.ValueOf(name))
		if mapValue.IsValid() && mapValue.Kind() == reflect.Interface {
			mapValue = mapValue.Elem()
		}
		if mapValue.IsValid() && mapValue.Kind() == reflect.String {
			return mapValue.String()
		}
	}
	return ""
}
