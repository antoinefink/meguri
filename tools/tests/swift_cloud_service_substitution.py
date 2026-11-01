"""Deterministic publication substitution probes; invoked only by focused Bats."""
import importlib.util
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
root = Path(os.environ['MG_ROOT'])
run = Path(os.environ['MG_RUN_DIR'])
source = Path(os.environ['FX_EXECUTABLE'])
spec = importlib.util.spec_from_file_location('helper', root / 'tools/scripts/swift-cloud-service-helper.py')
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)
(run / 'cloud-service-build.jsonl').write_text('{}\n')
original = helper.review.code_hash

for kind in ('source', 'destination'):
    calls = [0]

    def substituted(fd):
        value = original(fd)
        calls[0] += 1
        if (kind == 'source' and calls[0] == 1) or (kind == 'destination' and calls[0] == 3):
            target = source if kind == 'source' else run / 'cloud-service-code/meguri-cli'
            replacement = target.with_suffix('.replacement')
            replacement.write_bytes(target.read_bytes())
            replacement.chmod(0o755 if kind == 'source' else 0o500)
            os.replace(replacement, target)
        return value

    helper.review.code_hash = substituted
    try:
        helper.publish(run, source)
    except (OSError, ValueError):
        pass
    else:
        raise AssertionError('publication accepted a substituted binding')
    assert not (run / 'cloud-service-code.json').exists()
