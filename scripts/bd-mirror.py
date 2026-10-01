#!/usr/bin/env python3
"""Mirror `bd export` into <design>/issues/<id>.md so anneal reads beads as handles.

One-way and disposable: the directory is regenerated on every run and is
gitignored. bd stays the only writer. Usage:

    bd export | scripts/bd-mirror.py .design
"""
import json
import pathlib
import shutil
import sys


def quoted(value):
    return json.dumps(str(value or ""))


def main():
    out = pathlib.Path(sys.argv[1]) / "issues"
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    count = 0
    for line in sys.stdin:
        issue = json.loads(line)
        own = [d for d in issue.get("dependencies") or [] if d["issue_id"] == issue["id"]]
        by_type = {}
        for dep in own:
            by_type.setdefault(dep["type"], []).append(dep["depends_on_id"])
        front = [
            "---",
            f"bead: {issue['id']}",
            f"title: {quoted(issue.get('title') or issue['id'])}",
            f"status: {issue['status']}",
            f"priority: {issue.get('priority', '')}",
            f"updated: {(issue.get('updated_at') or '')[:10]}",
            f"close-reason: {quoted(issue.get('close_reason'))}",
        ]
        for parent in by_type.get("parent-child", [])[:1]:
            front.append(f"parent: issues/{parent}.md")
        if by_type.get("blocks"):
            front.append("depends-on: [" + ", ".join(f"issues/{b}.md" for b in by_type["blocks"]) + "]")
        if by_type.get("discovered-from"):
            front.append("discovered-from: [" + ", ".join(by_type["discovered-from"]) + "]")
        (out / f"{issue['id']}.md").write_text("\n".join(front) + f"\n---\n# {issue.get('title') or issue['id']}\n")
        count += 1
    print(f"mirrored {count} issues into {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
