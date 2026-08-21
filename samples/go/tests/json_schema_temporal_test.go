package tests

import (
	"fmt"
	"testing"
	"time"

	temporal "samples/go/temporal"

	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/converter"
)

// TestJSONSchemaTemporalRuntime round-trips the materialized temporal formats
// (date-time / date / time / duration) through the Temporal default data
// converter. Materialized fields are native Go types (time.Time / time.Duration)
// re-serialized via the generator-owned serializer: RFC 3339 with the original
// offset preserved, +00:00/-00:00 -> Z, trailing fractional zeros trimmed, and a
// time-only duration canonicalized to PT...H...M...S.
func TestJSONSchemaTemporalRuntime(t *testing.T) {
	dc := converter.GetDefaultDataConverter()

	// Canonical inputs (microsecond precision + offsets) round-trip byte-for-byte.
	full := roundTripJSONEq[temporal.Temporal](t, dc, "temporal", "temporal-full.json")
	require.Equal(t, 2021, full.CreatedAt.Year())
	_, off := full.CreatedAt.Zone()
	require.Equal(t, 2*3600, off) // +02:00 offset preserved
	require.Equal(t, 123456000, full.CreatedAt.Nanosecond())
	require.Equal(t, 90*time.Minute, full.Timeout)
	require.NotNil(t, full.DeletedAt)

	roundTripJSONEq[temporal.Temporal](t, dc, "temporal", "temporal-minimal.json")
}

// TestJSONSchemaTemporalCanonicalization decodes a non-canonical input (lowercase
// t/z, +00:00 offset, PT90M) and asserts the re-serialized form is canonicalized
// (uppercase T/Z, +00:00 -> Z, PT90M -> PT1H30M).
func TestJSONSchemaTemporalCanonicalization(t *testing.T) {
	dc := converter.GetDefaultDataConverter()
	canon := decodeFixture[temporal.Temporal](t, dc, "temporal", "temporal-canonicalize.json")
	got := reencode(t, dc, canon)
	require.JSONEq(
		t,
		`{"createdAt":"2021-06-15T12:30:45Z","birthday":"2021-02-28","alarm":"12:30:45Z","timeout":"PT1H30M"}`,
		string(got),
	)
}

// TestJSONSchemaTemporalNulls decodes optional+nullable temporals set to null.
// Go collapses optional+nullable (like the showcase middleName), so the explicit
// nulls decode to nil and are omitted on re-encode (verified by field checks).
func TestJSONSchemaTemporalNulls(t *testing.T) {
	dc := converter.GetDefaultDataConverter()
	nulls := decodeFixture[temporal.Temporal](t, dc, "temporal", "temporal-nulls.json")
	require.Nil(t, nulls.DeletedAt)
	require.Nil(t, nulls.ArchivedOn)
	require.Equal(t, time.Duration(0), nulls.Timeout) // PT0S
}

// TestJSONSchemaTemporalMaterializedNarrowing asserts the runtime parse adapter
// rejects the values the materialized grammar narrows away: leap second :60, a
// calendar (non-time-only) duration, and an invalid calendar date.
func TestJSONSchemaTemporalMaterializedNarrowing(t *testing.T) {
	dc := converter.GetDefaultDataConverter()
	base := `{"createdAt":%q,"birthday":%q,"alarm":%q,"timeout":%q}`
	cases := []struct {
		name                                      string
		createdAt, birthday, alarm, timeout, want string
	}{
		{"leap-second", "2021-12-31T23:59:60Z", "2000-01-01", "09:00:00", "PT0S", "date-time"},
		{"calendar-duration", "2021-06-15T12:30:45Z", "2000-01-01", "09:00:00", "P1Y", "duration"},
		{"invalid-date", "2021-06-15T12:30:45Z", "2021-02-29", "09:00:00", "PT0S", "date"},
		{"missing-offset", "2021-06-15T12:30:45", "2000-01-01", "09:00:00", "PT0S", "date-time"},
		{"year-zero-date-time", "0000-01-01T00:00:00Z", "2000-01-01", "09:00:00", "PT0S", "date-time"},
		{"year-zero-date", "2021-06-15T12:30:45Z", "0000-01-01", "09:00:00", "PT0S", "date"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			body := []byte(fmt.Sprintf(base, tc.createdAt, tc.birthday, tc.alarm, tc.timeout))
			var out temporal.Temporal
			err := dc.FromPayload(jsonPayload(body), &out)
			require.Error(t, err)
			require.Contains(t, err.Error(), tc.want)
		})
	}

	var firstYear temporal.Temporal
	err := dc.FromPayload(
		jsonPayload([]byte(`{"createdAt":"0001-01-01T00:00:00Z","birthday":"0001-01-01","alarm":"00:00:00","timeout":"PT0S"}`)),
		&firstYear,
	)
	require.NoError(t, err)
	require.Equal(t, 1, firstYear.CreatedAt.Year())
	require.Equal(t, 1, firstYear.Birthday.Year())
}
