#!/usr/bin/env python3
# Copyright 2021-Present Datadog, Inc.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Generates the binary seed corpora for the OTLP protobuf fuzz targets.

The protobuf wire format is encoded by hand rather than with a generated stub so
that the seeds can be regenerated without a protoc toolchain. Field numbers are
taken from the `#[prost(..., tag = "N")]` attributes in
`quickwit-proto/src/codegen/opentelemetry/`; if those protos are ever renumbered,
rerun this script.

Usage: python3 fuzz/scripts/generate_protobuf_seeds.py
"""

import struct
from pathlib import Path

WIRE_VARINT = 0
WIRE_FIXED64 = 1
WIRE_LENGTH_DELIMITED = 2
WIRE_FIXED32 = 5

TRACE_ID = bytes(range(1, 17))
SPAN_ID = bytes(range(1, 9))
START_TIME_UNIX_NANO = 1700000000000000000

SEVERITY_NUMBER_INFO = 9
SPAN_KIND_SERVER = 2
STATUS_CODE_OK = 1


def _varint(value: int) -> bytes:
    if value < 0:
        raise ValueError(f"negative values need zigzag or 10-byte encoding: {value}")
    out = bytearray()
    while True:
        byte = value & 0x7F
        value >>= 7
        out.append(byte | 0x80 if value else byte)
        if not value:
            return bytes(out)


def _tag(field_number: int, wire_type: int) -> bytes:
    return _varint((field_number << 3) | wire_type)


def _varint_field(field_number: int, value: int) -> bytes:
    return _tag(field_number, WIRE_VARINT) + _varint(value)


def _fixed64_field(field_number: int, value: int) -> bytes:
    return _tag(field_number, WIRE_FIXED64) + struct.pack("<Q", value)


def _fixed32_field(field_number: int, value: int) -> bytes:
    return _tag(field_number, WIRE_FIXED32) + struct.pack("<I", value)


def _bytes_field(field_number: int, value: bytes) -> bytes:
    return _tag(field_number, WIRE_LENGTH_DELIMITED) + _varint(len(value)) + value


def _string_field(field_number: int, value: str) -> bytes:
    return _bytes_field(field_number, value.encode("utf-8"))


def _message_field(field_number: int, message: bytes) -> bytes:
    return _bytes_field(field_number, message)


def _any_value_string(value: str) -> bytes:
    return _string_field(1, value)


def _any_value_int(value: int) -> bytes:
    return _varint_field(3, value)


def _key_value(key: str, any_value: bytes) -> bytes:
    return _string_field(1, key) + _message_field(2, any_value)


def _resource(attributes: bytes) -> bytes:
    return _message_field(1, attributes) + _varint_field(2, 0)


def _instrumentation_scope(name: str, version: str) -> bytes:
    return _string_field(1, name) + _string_field(2, version) + _varint_field(4, 0)


def _export_logs_service_request() -> bytes:
    log_record = (
        _fixed64_field(1, START_TIME_UNIX_NANO)
        + _varint_field(2, SEVERITY_NUMBER_INFO)
        + _string_field(3, "INFO")
        + _message_field(5, _any_value_string("hello from the fuzz corpus"))
        + _message_field(6, _key_value("http.status_code", _any_value_int(200)))
        + _varint_field(7, 0)
        + _fixed32_field(8, 0)
        + _bytes_field(9, TRACE_ID)
        + _bytes_field(10, SPAN_ID)
        + _fixed64_field(11, START_TIME_UNIX_NANO + 1)
    )
    scope_logs = _message_field(
        1, _instrumentation_scope("fuzz", "0.1.0")
    ) + _message_field(2, log_record)
    resource_logs = _message_field(
        1, _resource(_key_value("service.name", _any_value_string("quickwit-fuzz")))
    ) + _message_field(2, scope_logs)
    return _message_field(1, resource_logs)


def _export_trace_service_request() -> bytes:
    event = (
        _fixed64_field(1, START_TIME_UNIX_NANO + 500)
        + _string_field(2, "cache_miss")
        + _varint_field(4, 0)
    )
    link = (
        _bytes_field(1, TRACE_ID[::-1])
        + _bytes_field(2, SPAN_ID[::-1])
        + _varint_field(5, 0)
    )
    status = _string_field(2, "") + _varint_field(3, STATUS_CODE_OK)
    span = (
        _bytes_field(1, TRACE_ID)
        + _bytes_field(2, SPAN_ID)
        + _string_field(5, "GET /api/v1/search")
        + _varint_field(6, SPAN_KIND_SERVER)
        + _fixed64_field(7, START_TIME_UNIX_NANO)
        + _fixed64_field(8, START_TIME_UNIX_NANO + 1000)
        + _message_field(9, _key_value("http.method", _any_value_string("GET")))
        + _varint_field(10, 0)
        + _message_field(11, event)
        + _varint_field(12, 0)
        + _message_field(13, link)
        + _varint_field(14, 0)
        + _message_field(15, status)
    )
    scope_spans = _message_field(
        1, _instrumentation_scope("fuzz", "0.1.0")
    ) + _message_field(2, span)
    resource_spans = _message_field(
        1, _resource(_key_value("service.name", _any_value_string("quickwit-fuzz")))
    ) + _message_field(2, scope_spans)
    return _message_field(1, resource_spans)


def main() -> None:
    seeds_dir = Path(__file__).resolve().parent.parent / "seeds"
    seeds = {
        "otlp_logs_protobuf/single_log_record.binpb": _export_logs_service_request(),
        "otlp_traces_protobuf/single_span.binpb": _export_trace_service_request(),
    }
    for relative_path, payload in seeds.items():
        seed_path = seeds_dir / relative_path
        seed_path.parent.mkdir(parents=True, exist_ok=True)
        seed_path.write_bytes(payload)
        print(f"wrote {len(payload)} bytes to {seed_path}")


if __name__ == "__main__":
    main()
