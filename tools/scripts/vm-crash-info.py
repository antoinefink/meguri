#!/usr/bin/env python3
"""Retained Mac UI diagnostic only. No raw guest diagnostics cross SSH."""
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import signal
import stat
import subprocess
import sys

MAX_FILE = 8 * 1024 * 1024
MAX_REPORTS = 128
WINDOW = 60  # seconds either side of the admitted suite's process evidence
SIGNALS = frozenset(('SIGABRT', 'SIGSEGV', 'SIGBUS', 'SIGILL', 'SIGTRAP', 'SIGKILL'))
LIBRARIES = frozenset(('libsystem_kernel.dylib', 'libsystem_pthread.dylib',
                       'libsystem_c.dylib', 'libobjc.A.dylib', 'libdispatch.dylib',
                       'Foundation', 'CoreFoundation', 'AppKit', 'XCTest', 'DVTFoundation'))
COUNTS = ('candidates', 'matched', 'pid_mismatch', 'time_mismatch', 'link',
          'oversize', 'corrupt', 'changed', 'limit')
REFUSALS = frozenset(('input', 'source', 'evidence', 'changed', 'vm-unavailable',
                       'transport', 'guest-input', 'reports'))


class Refused(Exception):
    pass


def directory(path):
    """Open every path component without following a link (including parents)."""
    path = Path(path)
    if not path.is_absolute() or '..' in path.parts:
        raise Refused('source')
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in path.parts[1:]:
            nxt = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=fd)
            os.close(fd)
            fd = nxt
        return fd
    except BaseException:
        os.close(fd)
        raise


def identity(value):
    return (value.st_dev, value.st_ino, value.st_mode, value.st_nlink,
            value.st_size, value.st_mtime_ns, value.st_ctime_ns)


def regular(fd, name):
    before = os.stat(name, dir_fd=fd, follow_symlinks=False)
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
        raise Refused('link')
    if before.st_size > MAX_FILE:
        raise Refused('oversize')
    handle = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
    try:
        opened = os.fstat(handle)
        if identity(opened) != identity(before):
            raise Refused('changed')
        data = bytearray()
        while len(data) <= MAX_FILE:
            chunk = os.read(handle, min(65536, MAX_FILE + 1 - len(data)))
            if not chunk:
                break
            data.extend(chunk)
        if len(data) > MAX_FILE:
            raise Refused('oversize')
        if (identity(os.fstat(handle)) != identity(opened) or
                identity(os.stat(name, dir_fd=fd, follow_symlinks=False)) != identity(before)):
            raise Refused('changed')
        return bytes(data)
    finally:
        os.close(handle)


def timestamp(value):
    value = re.sub(r' (?=[+-]\d{2}:?\d{2}$)', '', value.strip()).replace('Z', '+00:00')
    parsed = dt.datetime.fromisoformat(value)
    if parsed.tzinfo is None:
        raise Refused('evidence')
    return parsed.timestamp()


def admit(root, run, suite):
    if not re.fullmatch(r'\d{8}T\d{6}Z-\d+', run) or not re.fullmatch(r'drive-[a-z0-9]+(?:-[a-z0-9]+)*', suite):
        raise Refused('input')
    root = str(Path(root))
    base = Path(root) / 'build/mg/mac-ui' / run
    hashes = []
    bindings = []
    contents = []
    for parent, name in ((base, 'result.json'), (base, 'log.txt'), (base / suite, 'drive.log')):
        fd = directory(parent)
        try:
            data = regular(fd, name)
            parent_stat = os.fstat(fd)
            bindings.append([parent_stat.st_dev, parent_stat.st_ino, parent_stat.st_mode,
                             *identity(os.stat(name, dir_fd=fd, follow_symlinks=False))])
        finally:
            os.close(fd)
        hashes.append(hashlib.sha256(data).hexdigest())
        contents.append(data)
    result = json.loads(contents[0])
    if (result.get('schema_version') != 1 or result.get('job') != 'mac-ui' or
            result.get('run_id') != run or result.get('root') != root or
            result.get('dry_run') is not False or result.get('placement') != 'host' or
            result.get('status') not in ('pass', 'warn', 'fail', 'error')):
        raise Refused('source')
    expected = 'drive:tools/drive-scripts/examples/' + suite[6:] + '.json'
    suites = result.get('details', {}).get('suites', [])
    if not isinstance(suites, list) or suites.count(expected) != 1:
        raise Refused('source')
    args = result.get('args')
    if not isinstance(args, list) or (args and args != sum((['--suite', s] for s in suites), [])):
        raise Refused('source')
    # The receipt must attribute its original log, never a user-supplied path.
    logs = [a.get('path') for a in result.get('artifacts', []) if a.get('kind') == 'log']
    if str(base / 'log.txt') not in logs or any(not isinstance(p, str) or not p.startswith(str(base) + '/') for p in logs):
        raise Refused('source')
    original_log = contents[1].decode('utf-8', errors='strict')
    if expected[6:] not in original_log or 'the Mac (vm)' not in original_log:
        raise Refused('evidence')
    start, end = timestamp(result['started_at']), timestamp(result['finished_at'])
    if not 0 <= end - start <= 24 * 3600:
        raise Refused('evidence')
    # Zoned evidence keeps the original, stricter process-point window.
    pattern = r'(?m)^(\d{4}-\d\d-\d\d[ T]\d\d:\d\d:\d\d(?:\.\d+)?(?:Z| ?[+-]\d\d:?\d\d))\s+xcodebuild\[(\d+):[^\]\n]+\]'
    text = contents[2].decode('utf-8', errors='strict')
    observations = [(timestamp(t), int(pid)) for t, pid in re.findall(pattern, text)]
    exact_one = not observations
    if observations:
        if len({p for _, p in observations}) != 1:
            raise Refused('evidence')
        if any(not start <= t <= end or not 1 <= p <= 2**31 - 1 for t, p in observations):
            raise Refused('evidence')
        first, last = min(t for t, _ in observations), max(t for t, _ in observations)
        if last - first > 300:
            raise Refused('evidence')
        pid, first, last = observations[0][1], first - WINDOW, last + WINDOW
    else:
        # Never parse or assign a zone to these local wall-time strings. Only
        # their original PID is evidence; the original UTC receipt bounds time.
        pids = {int(p) for p in re.findall(
            r'(?m)^\d{4}-\d\d-\d\d[ T]\d\d:\d\d:\d\d(?:\.\d+)?\s+xcodebuild\[(\d+):[^\]\n]+\]', text)}
        duration = result.get('duration_ms')
        if (len(pids) != 1 or not all(1 <= p <= 2**31 - 1 for p in pids) or
                not 0 < end - start <= 420 or type(duration) is not int or
                not 0 < duration <= 420000 or
                any(not re.search(r'(?:Z|\+00:00)$', result[k])
                    for k in ('started_at', 'finished_at'))):
            raise Refused('evidence')
        pid, first, last = pids.pop(), start, end
    vm = result.get('details', {}).get('mac_vm', {}).get('name')
    if not isinstance(vm, str) or not re.fullmatch(r'[A-Za-z0-9._-]+', vm):
        raise Refused('evidence')
    return dict(hashes=hashes, bindings=bindings, vm=vm, pid=pid,
                start=first, end=last, exact_one=exact_one)


def objects(data):
    decoder = json.JSONDecoder()
    text = data.decode('utf-8', errors='strict')
    values = []
    while text.strip():
        value, offset = decoder.raw_decode(text.lstrip())
        values.append(value)
        text = text.lstrip()[offset:]
        if len(values) > 2 or not isinstance(value, dict):
            raise Refused('corrupt')
    if not values:
        raise Refused('corrupt')
    return values


def library(name, path):
    # Only actual Apple system image locations qualify for the fixed taxonomy.
    if (not isinstance(path, str) or '..' in Path(path).parts or
            not path.startswith(('/System/Library/', '/usr/lib/'))):
        return 'other'
    basename = path.rsplit('/', 1)[-1]
    return basename if basename in LIBRARIES and name == basename else 'other'


def ips(data):
    values = objects(data)
    body = values[-1]
    if body.get('procName') != 'xcodebuild' or type(body.get('pid')) is not int:
        raise Refused('corrupt')
    when = timestamp(body['captureTime'])
    signal = body.get('exception', {}).get('signal', 'other')
    signal = signal if isinstance(signal, str) and signal in SIGNALS else 'other'
    index = body.get('faultingThread')
    threads, images = body.get('threads'), body.get('usedImages')
    if (type(index) is not int or not isinstance(threads, list) or not 0 <= index < len(threads)
            or not isinstance(images, list)):
        raise Refused('corrupt')
    frames = threads[index].get('frames')
    if not isinstance(frames, list) or not frames:
        raise Refused('corrupt')
    image_index = frames[0].get('imageIndex')
    if type(image_index) is not int or not 0 <= image_index < len(images):
        raise Refused('corrupt')
    image = images[image_index]
    return body['pid'], when, signal, library(image.get('name'), image.get('path'))


def crash(data):
    text = data.decode('utf-8', errors='strict')
    process = re.search(r'(?m)^Process:\s+xcodebuild \[(\d+)\]\s*$', text)
    when = re.search(r'(?m)^Date/Time:\s*(.+)$', text)
    thread = re.search(r'(?m)^Crashed Thread:\s*(\d+)(?:\s|$)', text)
    if not process or not when or not thread:
        raise Refused('corrupt')
    # A frame-zero address must resolve to the actual Binary Images entry.
    frame = re.search(r'(?m)^Thread ' + thread[1] + r' Crashed:[^\n]*\n0\s+(\S+)\s+(0x[0-9a-fA-F]+)\s', text)
    if not frame:
        raise Refused('corrupt')
    address = int(frame[2], 16)
    matches = []
    for low, high, name, path in re.findall(r'(?m)^\s*(0x[\da-fA-F]+)\s*-\s*(0x[\da-fA-F]+)\s+(\S+)\s+[^\n]*?(/[^\n]+)$', text):
        if int(low, 16) <= address <= int(high, 16) and name.lstrip('+') == frame[1]:
            matches.append(library(frame[1], path))
    if len(matches) != 1:
        raise Refused('corrupt')
    signal = re.search(r'(?m)^Exception Type:[^\n]*\((SIG[A-Z]+)\)', text)
    signal = signal[1] if signal and signal[1] in SIGNALS else 'other'
    return int(process[1]), timestamp(when[1]), signal, matches[0]


def scan(path, admission):
    counts = dict.fromkeys(COUNTS, 0)
    facts = []
    try:
        fd = directory(path)
    except FileNotFoundError:
        refused = admission.get('exact_one', False)
        return dict(outcome='refused' if refused else 'no-evidence',
                    category='reports' if refused else 'none', counts=counts, reports=[])
    try:
        # Names never leave the guest. Bound iteration as well as candidate bytes.
        names = []
        with os.scandir(fd) as entries:
            for i, entry in enumerate(entries):
                if i >= 4096:
                    counts['limit'] += 1
                    break
                if entry.name.startswith('xcodebuild') and entry.name.endswith(('.ips', '.crash')):
                    names.append(entry.name)
                    if len(names) > MAX_REPORTS:
                        counts['limit'] += 1
                        break
        for name in sorted(names[:MAX_REPORTS]):
            counts['candidates'] += 1
            try:
                data = regular(fd, name)
                pid, when, signal, image = (ips if name.endswith('.ips') else crash)(data)
                if pid != admission['pid']:
                    counts['pid_mismatch'] += 1
                elif not admission['start'] <= when <= admission['end']:
                    counts['time_mismatch'] += 1
                else:
                    counts['matched'] += 1
                    facts.append(dict(process='xcodebuild', signal=signal, library=image))
            except Refused as exc:
                category = str(exc)
                counts[category if category in ('link', 'oversize', 'changed') else 'corrupt'] += 1
            except (ValueError, KeyError, TypeError, AttributeError, IndexError, OSError, RecursionError):
                counts['corrupt'] += 1
    finally:
        os.close(fd)
    ambiguous = admission.get('exact_one', False) and counts['matched'] != 1
    refused = ambiguous or any(counts[k] for k in ('link', 'oversize', 'corrupt', 'changed', 'limit'))
    return dict(outcome='refused' if refused else ('observed' if facts else 'no-evidence'),
                category='reports' if refused else 'none', counts=counts, reports=[] if ambiguous else facts)


def validate_public(value, exact_one=False):
    if not isinstance(value, dict) or set(value) != {'outcome', 'category', 'counts', 'reports'}:
        raise Refused('transport')
    if value['outcome'] not in ('observed', 'no-evidence', 'refused') or value['category'] not in ('reports', 'none'):
        raise Refused('transport')
    counts = value['counts']
    if not isinstance(counts, dict) or set(counts) != set(COUNTS) or any(type(n) is not int or not 0 <= n <= MAX_REPORTS + 1 for n in counts.values()):
        raise Refused('transport')
    reports = value['reports']
    ambiguous = exact_one and counts['matched'] != 1
    if not isinstance(reports, list) or len(reports) != (0 if ambiguous else counts['matched']):
        raise Refused('transport')
    for fact in reports:
        if not isinstance(fact, dict) or set(fact) != {'process', 'signal', 'library'} or fact['process'] != 'xcodebuild' or fact['signal'] not in SIGNALS | {'other'} or fact['library'] not in LIBRARIES | {'other'}:
            raise Refused('transport')
    refused = ambiguous or any(counts[k] for k in ('link', 'oversize', 'corrupt', 'changed', 'limit'))
    expected = 'refused' if refused else ('observed' if reports else 'no-evidence')
    if value['outcome'] != expected or value['category'] != ('reports' if refused else 'none'):
        raise Refused('transport')
    if sum(counts[k] for k in COUNTS if k not in ('candidates', 'limit')) != counts['candidates']:
        raise Refused('transport')
    return value


def public(run, suite, value):
    return dict(scope='MAC-UI-CRASH-DIAGNOSTIC-ONLY', source_job='mac-ui',
                source='original-result-and-suite-log', run=run, suite=suite, **value)


def owned_cleanup():
    spec = importlib.util.spec_from_file_location(
        'crash_owned_cleanup', Path(__file__).with_name('vm-xcode-stage.py'))
    owned = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(owned)
    return owned


def collect(context, admission):
    # Reuse the existing original-session launch deferral, bounded TERM/KILL
    # group cleanup and file-backed output. No new transport/cleanup framework.
    owned = owned_cleanup()
    saved = {s: signal.signal(s, lambda *_: (_ for _ in ()).throw(owned.Cancelled()))
             for s in (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)}
    try:
        command = shlex.join(('/usr/bin/python3', '-B', '-I', '-', '--guest',
                              str(admission['pid']), str(admission['start']),
                              str(admission['end']), 'one' if admission['exact_one'] else 'zoned'))
        _, raw = owned.command([context['selection']['ssh'], command],
                               data=Path(__file__).read_bytes(), timeout=60)
        if len(raw) > 65536:
            raise Refused('transport')
        value = validate_public(json.loads(raw), admission['exact_one'])
        # This flag belongs only to this original foreground guest invocation.
        # Local joins, timeout/cancellation and later probes never create it.
        with owned.finishing():
            fd = os.open(Path(context['private']) / 'lease-safe.json',
                         os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, 'w') as output:
                json.dump({'safe': True}, output)
        return value
    except (owned.Refused, owned.Cancelled, KeyboardInterrupt):
        raise Refused('transport') from None
    finally:
        for sig, handler in saved.items():
            signal.signal(sig, handler)


def main():
    if len(sys.argv) == 6 and sys.argv[1] == '--guest':
        try:
            pid, start, end = int(sys.argv[2]), float(sys.argv[3]), float(sys.argv[4])
            if (not 1 <= pid <= 2**31 - 1 or not 0 < start <= end or end - start > 420
                    or sys.argv[5] not in ('one', 'zoned')):
                raise Refused('guest-input')
            value = scan(Path.home() / 'Library/Logs/DiagnosticReports',
                         dict(pid=pid, start=start, end=end, exact_one=sys.argv[5] == 'one'))
            print(json.dumps(value))
            return 0
        except Exception:
            # Do not print exception text, paths, stacks or account information.
            return 1
    if len(sys.argv) != 5 or sys.argv[1] not in ('--admit', '--collect'):
        return 1
    mode, root, run, suite = sys.argv[1:]
    try:
        admission = admit(root, run, suite)
        if mode == '--admit':
            print(json.dumps(admission))
            return 0
        with os.fdopen(3) as context_file:
            context = json.load(context_file)
        if context['admission'] != admission:
            raise Refused('changed')
        value = collect(context, admission)
        if admit(root, run, suite) != admission:
            raise Refused('changed')
        print(json.dumps(public(run, suite, value)))
        return 1 if value['outcome'] == 'refused' else 0
    except Refused as exc:
        category = str(exc) if str(exc) in REFUSALS else 'source'
    except (OSError, ValueError, KeyError, TypeError, AttributeError, RecursionError, subprocess.SubprocessError):
        category = 'source' if mode == '--admit' else 'transport'
    if mode == '--collect':
        print(json.dumps(public(run, suite, dict(outcome='refused', category=category,
                                               counts=dict.fromkeys(COUNTS, 0), reports=[]))))
    return 1


if __name__ == '__main__':
    sys.exit(main())
