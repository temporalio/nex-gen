using System;
using System.Linq;
using System.Reflection;
using Google.Protobuf;
using Temporalio.Converters;
using Payload = Temporalio.Api.Common.V1.Payload;

namespace NexGen.Support
{
    public interface ITemporalWire
    {
        object TemporalToWire(IPayloadConverter? payloadConverter = null);
    }

    public sealed class TemporalWirePayloadConverter : DefaultPayloadConverter
    {
        public TemporalWirePayloadConverter()
            : base(WithTemporalWireConverter())
        {
        }

        private static IEncodingConverter[] WithTemporalWireConverter()
        {
            var inner = new DefaultPayloadConverter(
                new DefaultPayloadConverter()
                    .EncodingConverters
                    .Where(converter => converter.Encoding != "json/protobuf")
                    .ToArray());
            return new[] { new TemporalWireEncodingConverter(inner) }
                .Concat(inner.EncodingConverters)
                .ToArray();
        }
    }

    public sealed class TemporalWireEncodingConverter : IEncodingConverter
    {
        private const string InnerEncodingMetadataKey = "temporal-wire-encoding";
        private const string InnerMetadataPrefix = "temporal-wire-metadata-";
        private static readonly ByteString EncodingBytes = ByteString.CopyFromUtf8("binary/temporal-wire");
        private readonly IPayloadConverter inner;

        public TemporalWireEncodingConverter(IPayloadConverter inner)
        {
            this.inner = inner;
        }

        public string Encoding => "binary/temporal-wire";

        public bool TryToPayload(object? value, out Payload? payload)
        {
            if (value is not ITemporalWire temporalWire)
            {
                payload = default!;
                return false;
            }
            var wirePayload = inner.ToPayload(temporalWire.TemporalToWire(inner));
            payload = new Payload
            {
                Data = wirePayload.Data,
            };
            foreach (var item in wirePayload.Metadata)
            {
                if (item.Key == "encoding")
                {
                    payload.Metadata[InnerEncodingMetadataKey] = item.Value;
                }
                else
                {
                    payload.Metadata[InnerMetadataPrefix + item.Key] = item.Value;
                }
            }
            payload.Metadata["encoding"] = EncodingBytes;
            return true;
        }

        public object? ToValue(Payload payload, Type type)
        {
            var fromWire = type.GetMethod(
                "TemporalFromWire",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);
            if (fromWire == null)
            {
                throw new ArgumentException(
                    $"Type {type} is missing Temporal wire factory method.");
            }
            var parameters = fromWire.GetParameters();
            if (parameters.Length == 0)
            {
                throw new ArgumentException(
                    $"Type {type} has an invalid Temporal wire factory method.");
            }
            if (!payload.Metadata.TryGetValue(InnerEncodingMetadataKey, out var innerEncoding))
            {
                throw new ArgumentException("Temporal wire payload is missing inner encoding metadata.");
            }
            var innerPayload = new Payload
            {
                Data = payload.Data,
            };
            innerPayload.Metadata["encoding"] = innerEncoding;
            foreach (var item in payload.Metadata)
            {
                if (item.Key.StartsWith(InnerMetadataPrefix, StringComparison.Ordinal))
                {
                    innerPayload.Metadata[item.Key.Substring(InnerMetadataPrefix.Length)] = item.Value;
                }
            }
            var wireValue = inner.ToValue(innerPayload, parameters[0].ParameterType);
            return fromWire.Invoke(null, new[] { wireValue, inner });
        }
    }
}
