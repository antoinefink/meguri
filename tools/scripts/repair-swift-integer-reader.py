#!/usr/bin/env python3
"""KTD9/R1802: repair only the pinned UniFFI Swift integer reader."""

import re
import sys
from pathlib import Path


SIGNATURE = b"fileprivate func readInt<T: FixedWidthInteger>(_ reader: inout (data: Data, offset: Data.Index)) throws -> T"
OLD = SIGNATURE + b""" {
    let range = reader.offset..<reader.offset + MemoryLayout<T>.size
    guard reader.data.count >= range.upperBound else {
        throw UniffiInternalError.bufferOverflow
    }
    if T.self == UInt8.self {
        let value = reader.data[reader.offset]
        reader.offset += 1
        return value as! T
    }
    var value: T = 0
    let _ = withUnsafeMutableBytes(of: &value, { reader.data.copyBytes(to: $0, from: range)})
    reader.offset = range.upperBound
    return value.bigEndian
}"""
NEW = b"""fileprivate func readInt<T: FixedWidthInteger & BitwiseCopyable>(_ reader: inout (data: Data, offset: Data.Index)) throws -> T {
    // RustBuffer readers use zero-based Data; raw-buffer offsets are relative.
    guard reader.data.startIndex == 0 else {
        throw UniffiInternalError.bufferOverflow
    }
    let offset = reader.offset
    let size = MemoryLayout<T>.size
    guard offset >= 0, offset <= reader.data.count,
          size <= reader.data.count - offset else {
        throw UniffiInternalError.bufferOverflow
    }
    let value = reader.data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
        bytes.loadUnaligned(fromByteOffset: offset, as: T.self)
    }
    reader.offset = offset + size
    return value.bigEndian
}"""


def repair(source: bytes) -> bytes:
    if (len(re.findall(rb"\bfunc\s+readInt\b", source)) != 1
            or source.count(SIGNATURE) != 1
            or source.count(OLD) != 1
            or source.count(b"fileprivate func readBytes(") != 1):
        raise ValueError("expected exactly one exact pinned readInt helper/signature and readBytes helper")
    return source.replace(OLD, NEW, 1)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: repair-swift-integer-reader.py <generated.swift>")
    path = Path(sys.argv[1])
    try:
        repaired = repair(path.read_bytes())
        path.write_bytes(repaired)
    except (OSError, ValueError) as error:
        sys.exit(f"Swift integer reader repair refused {path}: {error}")
