"""Check the paths and order changes contributed by an art/<id> branch."""

import argparse
import json
import re
import subprocess
import sys


QUEUE = "art_pipeline/QUEUE.json"
CREATURE = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")
REGULAR_MODES = {"000000", "100644", "100755"}
NEXT_STATUS = {
    "TODO": "IN_PROGRESS",
    "IN_PROGRESS": "READY_FOR_INTEGRATION",
    "READY_FOR_INTEGRATION": None,
    "INTEGRATED": None,
}
UNSAFE_COMPONENT = re.compile(
    r'[\\:*?"<>|\x00-\x1f]|[. ]$|^(?:\.git|git~1|con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)',
    re.IGNORECASE,
)


class GuardError(Exception):
    pass


def git(*args):
    result = subprocess.run(
        ["git", "--no-replace-objects", *args], capture_output=True, check=False
    )
    if result.returncode:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise GuardError(f"git {args[0]} failed: {detail}")
    return result.stdout


def commit_id(ref):
    return git("rev-parse", "--verify", "--end-of-options", f"{ref}^{{commit}}").decode().strip()


def verify_paths(start, end, creature):
    exact = {QUEUE, f"art_pipeline/reports/{creature}.md", f"assets/creatures/{creature}_art.json"}
    prefixes = (
        f"art_pipeline/previews/{creature}/",
        f"assets/creatures/{creature}_art/",
        f"tools/blender/{creature}/",
    )
    # A rename appears as a deletion and an addition, so both names are checked.
    entries = git(
        "diff", "--raw", "-z", "--no-renames", "--ignore-submodules=none",
        start, end, "--",
    ).split(b"\0")
    for metadata, raw_path in zip(entries[0:-1:2], entries[1:-1:2]):
        try:
            path = raw_path.decode("utf-8")
        except UnicodeError as error:
            raise GuardError(f"Non-UTF-8 path: {raw_path!r}") from error
        if any(not part or UNSAFE_COMPONENT.search(part) for part in path.split("/")):
            raise GuardError(f"Path cannot be used safely on Windows: {path!r}")
        if path not in exact and not path.startswith(prefixes):
            raise GuardError(f"Path outside art/{creature}: {path}")
        modes = metadata[1:].decode("ascii").split()[:2]
        if any(mode not in REGULAR_MODES for mode in modes):
            raise GuardError(f"Only regular files are allowed: {path} ({', '.join(modes)})")


def unique_fields(pairs):
    fields = {}
    for key, value in pairs:
        if key in fields:
            raise ValueError(f"duplicate key {key!r}")
        fields[key] = value
    return fields


def reject_constant(value):
    raise ValueError(f"invalid JSON number {value!r}")


def read_queue(commit, creature):
    try:
        queue = json.loads(
            git("show", f"{commit}:{QUEUE}").decode("utf-8"),
            object_pairs_hook=unique_fields,
            parse_constant=reject_constant,
        )
    except (ValueError, RecursionError) as error:
        raise GuardError(f"{QUEUE} in {commit[:12]}: {error}") from error
    if (
        not isinstance(queue, dict)
        or type(queue.get("schema_version")) is not int
        or queue["schema_version"] != 1
        or not isinstance(queue.get("orders"), list)
    ):
        raise GuardError(f"{QUEUE} in {commit[:12]} must use schema_version 1 and an orders list")
    ids = set()
    current = None
    for order in queue["orders"]:
        order_id = order.get("id") if isinstance(order, dict) else None
        if not isinstance(order_id, str) or not CREATURE.fullmatch(order_id) or order_id in ids:
            raise GuardError(f"{QUEUE} in {commit[:12]} has an invalid or duplicate order id")
        ids.add(order_id)
        if order_id == creature:
            current = order
    if current is None:
        raise GuardError(f"{QUEUE} in {commit[:12]} has no order for {creature}")
    status = current.get("status")
    if not isinstance(status, str) or status not in NEXT_STATUS:
        raise GuardError(f"{QUEUE} has an invalid status for {creature}: {status!r}")
    if not isinstance(current.get("notes"), str):
        raise GuardError(f"{QUEUE} requires a notes string for {creature}")
    return queue, current


def canonical(value):
    # Unlike Python equality, this distinguishes JSON true, 1 and 1.0.
    return json.dumps(value, sort_keys=True)


def verify_queue_fields(before, previous, after, current, creature):
    if canonical({k: v for k, v in before.items() if k != "orders"}) != canonical(
        {k: v for k, v in after.items() if k != "orders"}
    ):
        raise GuardError(f"{QUEUE} root fields must stay unchanged")
    if [order["id"] for order in before["orders"]] != [order["id"] for order in after["orders"]]:
        raise GuardError(f"{QUEUE} order identities and order of entries must stay unchanged")
    for old_order, new_order in zip(before["orders"], after["orders"]):
        if old_order["id"] != creature and canonical(old_order) != canonical(new_order):
            raise GuardError(f"{QUEUE} order {old_order['id']} does not belong to art/{creature}")
    for field in sorted(previous.keys() | current.keys()):
        if field in {"status", "notes"}:
            continue
        if (
            field not in previous
            or field not in current
            or canonical(previous[field]) != canonical(current[field])
        ):
            raise GuardError(f"{QUEUE} order {creature} cannot change field {field}")


def verify_commit(commit, parents, art_commits, creature):
    if len(parents) == 1:
        reference = parents[0]
        art_parent = None
    elif len(parents) == 2 and sum(parent in art_commits for parent in parents) == 1:
        # Accept a merge from the trusted target, comparing the resulting tree
        # against that target so trusted updates pass through unchanged.
        reference = next(parent for parent in parents if parent not in art_commits)
        art_parent = next(parent for parent in parents if parent in art_commits)
    else:
        raise GuardError("Merge must have one art parent and one trusted target parent")

    verify_paths(reference, commit, creature)
    before, previous = read_queue(reference, creature)
    after, current = read_queue(commit, creature)
    verify_queue_fields(before, previous, after, current, creature)
    previous_status = previous["status"]
    if art_parent is not None:
        _, art_order = read_queue(art_parent, creature)
        statuses = list(NEXT_STATUS)
        previous_status = max(previous_status, art_order["status"], key=statuses.index)
    if current["status"] not in {previous_status, NEXT_STATUS[previous_status]}:
        raise GuardError(
            f"{QUEUE} order {creature}: forbidden status transition "
            f"{previous_status} -> {current['status']}"
        )


def check_art_branch(base, head, branch):
    creature = branch.removeprefix("art/")
    if not branch.startswith("art/") or not CREATURE.fullmatch(creature):
        raise GuardError("Branch must be art/<id>, with lowercase letters, digits and single hyphens")

    base = commit_id(base)
    head = commit_id(head)
    ancestors = git("merge-base", "--all", base, head).decode().splitlines()
    if len(ancestors) != 1:
        raise GuardError("Base and head must share a single merge base")
    verify_paths(ancestors[0], head, creature)
    read_queue(base, creature)

    # Check every contributed commit, even when a later commit cancels its diff.
    history = git("rev-list", "--reverse", "--topo-order", "--parents", f"{base}..{head}")
    commits = [line.split() for line in history.decode().splitlines()]
    art_commits = {commit for commit, *_ in commits}
    for commit, *parents in commits:
        try:
            verify_commit(commit, parents, art_commits, creature)
        except GuardError as error:
            raise GuardError(f"Commit {commit[:12]}: {error}") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, help="trusted target commit or ref, e.g. origin/main")
    parser.add_argument("--head", required=True, help="art commit or ref to check, e.g. HEAD")
    parser.add_argument("--branch", required=True, help="source branch name, e.g. art/wyvern")
    args = parser.parse_args()
    try:
        check_art_branch(args.base, args.head, args.branch)
    except (GuardError, OSError) as error:
        print(f"Art branch guard rejected {args.branch!r}: {error}", file=sys.stderr)
        return 1
    print(f"Art branch guard accepted {args.branch}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
