#!/usr/bin/env python3
"""Refuse instrumented Profile Performance xctestruns; never modify them."""
import json
import plistlib
import sys


FLAGS = ("UITargetAppMainThreadCheckerEnabled",
         "UITargetAppPerformanceAntipatternCheckerEnabled")


def inspect(path):
    # Fixed-size aggregates only: no target names, paths or environment values.
    report = {
        "status": "fail", "reason": "unavailable", "ui_targets": 0,
        "flags": {key: dict(false=0, true=0, missing=0, invalid=0) for key in FLAGS},
        "environment_dictionaries": 0,
        "main_thread_checker_injections": 0,
        "performance_antipattern_checker_injections": 0,
    }
    try:
        with open(path, "rb") as source:
            data = plistlib.load(source)
    except OSError:
        return report
    except Exception:
        report["reason"] = "malformed"
        return report

    def environments(node):
        if isinstance(node, dict):
            for key, value in node.items():
                if key.endswith("EnvironmentVariables"):
                    if not isinstance(value, dict) or not all(isinstance(v, str) for v in value.values()):
                        raise ValueError("environment dictionary")
                    report["environment_dictionaries"] += 1
                    for entry in value.values():
                        report["main_thread_checker_injections"] += int("mainthreadchecker" in entry.lower())
                        report["performance_antipattern_checker_injections"] += int("librpac" in entry.lower())
                else:
                    environments(value)
        elif isinstance(node, list):
            for value in node:
                environments(value)

    try:
        if not isinstance(data, dict):
            raise ValueError("root dictionary")
        if "TestConfigurations" in data:
            configurations = data["TestConfigurations"]
            if not isinstance(configurations, list) or not configurations:
                raise ValueError("configurations")
            targets = []
            for configuration in configurations:
                if not isinstance(configuration, dict) or not isinstance(configuration.get("TestTargets"), list):
                    raise ValueError("targets")
                targets.extend(configuration["TestTargets"])
        else:
            # Format 1 stores each target directly under the root.
            targets = [value for key, value in data.items() if key != "__xctestrun_metadata__"]
        for target in targets:
            if not isinstance(target, dict):
                raise ValueError("target dictionary")
            if "IsUITestBundle" in target and type(target["IsUITestBundle"]) is not bool:
                raise ValueError("UI target marker")
            if target.get("IsUITestBundle") is not True and "UITargetAppPath" not in target:
                continue
            report["ui_targets"] += 1
            for key in FLAGS:
                value = target.get(key)
                state = ("missing" if key not in target else "false" if value is False
                         else "true" if value is True else "invalid")
                report["flags"][key][state] += 1
        environments(data)
    except Exception:
        report["reason"] = "malformed"
        return report
    report["reason"] = "policy"
    if (report["ui_targets"] > 0
            and all(report["flags"][key]["false"] == report["ui_targets"] for key in FLAGS)
            and report["main_thread_checker_injections"] == 0
            and report["performance_antipattern_checker_injections"] == 0):
        report.update(status="pass", reason="uninstrumented")
    return report


if __name__ == "__main__":
    result = inspect(sys.argv[1] if len(sys.argv) == 2 else "")
    print(json.dumps(result, sort_keys=True))
    sys.exit(0 if result["status"] == "pass" else 1)
