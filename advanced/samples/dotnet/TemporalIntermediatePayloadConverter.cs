using System;
using System.Reflection;
using Temporalio.Converters;
using Payload = Temporalio.Api.Common.V1.Payload;

namespace Nexgen.Support
{
    public interface ITemporalIntermediate
    {
        object TemporalToIntermediate(IPayloadConverter? payloadConverter = null);
    }

    public sealed class TemporalIntermediatePayloadConverter : IPayloadConverter
    {
        private readonly IPayloadConverter inner;

        public TemporalIntermediatePayloadConverter(IPayloadConverter? inner = null)
        {
            this.inner = inner ?? new DefaultPayloadConverter();
        }

        public Payload ToPayload(object? value)
        {
            if (value is ITemporalIntermediate temporalIntermediate)
            {
                value = temporalIntermediate.TemporalToIntermediate(inner);
            }
            return inner.ToPayload(value);
        }

        public object? ToValue(Payload payload, Type type)
        {
            var fromIntermediate = type.GetMethod(
                "TemporalFromIntermediate",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);
            if (fromIntermediate == null)
            {
                return inner.ToValue(payload, type);
            }
            var parameters = fromIntermediate.GetParameters();
            if (parameters.Length == 0)
            {
                throw new ArgumentException(
                    $"Type {type} has an invalid Temporal intermediate factory method.");
            }
            if (parameters.Length > 2)
            {
                throw new ArgumentException(
                    $"Type {type} has an invalid Temporal intermediate factory method.");
            }
            var intermediateValue = inner.ToValue(payload, parameters[0].ParameterType);
            if (parameters.Length == 1)
            {
                return fromIntermediate.Invoke(null, new[] { intermediateValue });
            }
            return fromIntermediate.Invoke(null, new[] { intermediateValue, inner });
        }
    }
}
