# SPDX-License-Identifier: AGPL-3.0-or-later
"""Protobuf messages -> neutral observation values.

- f32 fields become their shortest round-trip f32 decimal (numbers.f32_value).
- A oneof named `kind` is flattened: the active member's name becomes "kind"
  and its fields are inlined; a nested `kind` (a wrapper such as Normal) wins,
  so a learning state reads {"kind": "learning", "remaining_steps": ...}.
- Fields with presence (messages, proto3 `optional`) appear only when set;
  plain scalars always appear, defaults included.
- Enums are lower-case value names; bytes are lower-case hex.
"""

from __future__ import annotations

from typing import Any

from google.protobuf.descriptor import FieldDescriptor as FD
from google.protobuf.message import Message

from .numbers import f32_value


def _scalar(fd: FD, value: Any) -> Any:
    if fd.type == FD.TYPE_FLOAT:
        return f32_value(value)
    if fd.type == FD.TYPE_ENUM:
        return fd.enum_type.values_by_number[value].name.lower() if value in fd.enum_type.values_by_number else value
    if fd.type == FD.TYPE_BYTES:
        return value.hex()
    if fd.type == FD.TYPE_MESSAGE:
        return message(value)
    return value


def _real_oneof(fd: FD):
    """The field's oneof, unless it is the synthetic one of a proto3 `optional`."""
    oneof = fd.containing_oneof
    if oneof is None or (len(oneof.fields) == 1 and oneof.name == "_" + fd.name):
        return None
    return oneof


def _value(fd: FD, value: Any) -> Any:
    if fd.is_repeated:
        if fd.message_type is not None and fd.message_type.GetOptions().map_entry:
            vfd = fd.message_type.fields_by_name["value"]
            return {str(k): _scalar(vfd, v) for k, v in value.items()}
        return [_scalar(fd, v) for v in value]
    return _scalar(fd, value)


def message(msg: Message) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for fd in msg.DESCRIPTOR.fields:
        oneof = _real_oneof(fd)
        if oneof is not None:
            if msg.WhichOneof(oneof.name) != fd.name:
                continue
            value = _value(fd, getattr(msg, fd.name))
            if oneof.name == "kind" and isinstance(value, dict):
                out["kind"] = fd.name
                out.update(value)
            else:
                out[fd.name] = value
            continue
        if fd.has_presence and not msg.HasField(fd.name):
            continue
        out[fd.name] = _value(fd, getattr(msg, fd.name))
    return out
