"""Exercise the art branch guard against real Git history."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


GUARD = Path(__file__).with_name("check-art-branch.py").resolve()
QUEUE = "art_pipeline/QUEUE.json"
ASSET = "assets/creatures/wyvern_art/model.glb"
SPEC = "art_pipeline/specs/wyvern.md"


def order(creature, status="TODO"):
    return {
        "id": creature,
        "priority": 1,
        "status": status,
        "spec_path": f"art_pipeline/specs/{creature}.md",
        "manifest_path": f"assets/creatures/{creature}_art.json",
        "output_path": f"assets/creatures/{creature}_art/",
        "replaces": "placeholder",
        "assigned_to": "PC2-ZCode",
        "notes": "Original notes",
    }


class GuardTests(unittest.TestCase):
    def setUp(self):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        self.repo = Path(work.name)
        self.git("init", "-q", "-b", "main")
        self.original = {"schema_version": 1, "orders": [order("wyvern"), order("raptor")]}
        self.write_queue(self.original)
        for path in (SPEC, "AGENTS.md", "crates/example/src/lib.rs", ASSET):
            self.write(path, "original content\n")
        self.commit()
        self.git("switch", "-q", "-c", "art/wyvern")

    def git(self, *args, input=None):
        return subprocess.run(
            ["git", "-c", "user.name=Guard Test", "-c", "user.email=guard@example.test", *args],
            cwd=self.repo,
            input=input,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()

    def write(self, path, content):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")

    def write_queue(self, queue):
        self.write(QUEUE, json.dumps(queue, indent=2))

    def set_status(self, status, index=0):
        queue = json.loads((self.repo / QUEUE).read_text(encoding="utf-8"))
        queue["orders"][index]["status"] = status
        self.write_queue(queue)
        self.commit()

    def commit(self, *, stage=True):
        if stage:
            self.git("add", "--all")
        self.git("commit", "-q", "-m", "Test snapshot")

    def guard(self, *, branch="art/wyvern", expected=None):
        result = subprocess.run(
            [sys.executable, str(GUARD), "--base", "main", "--head", "HEAD", "--branch", branch],
            cwd=self.repo,
            capture_output=True,
            text=True,
        )
        details = result.stdout + result.stderr
        if expected is None:
            self.assertEqual(result.returncode, 0, details)
        else:
            self.assertNotEqual(result.returncode, 0, details)
            self.assertIn(expected, details)

    def test_all_art_paths_and_sequential_status_are_accepted(self):
        self.set_status("IN_PROGRESS")
        for path in (
            "art_pipeline/reports/wyvern.md",
            "art_pipeline/previews/wyvern/pose.png",
            "assets/creatures/wyvern_art.json",
            "assets/creatures/wyvern_art/textures/body.png",
            "tools/blender/wyvern/build.py",
        ):
            self.write(path, "asset content\n")
        self.git("mv", ASSET, ASSET.replace("model.glb", "body.glb"))
        queue = copy.deepcopy(self.original)
        queue["orders"][0]["status"] = "READY_FOR_INTEGRATION"
        queue["orders"][0]["notes"] = "Preview rendered."
        self.write(QUEUE, json.dumps(queue, sort_keys=True))
        self.commit()
        self.guard()

    def test_each_protected_path_is_rejected(self):
        for path in (
            "crates/example/src/lib.rs",
            "AGENTS.md",
            SPEC,
            ".github/workflows/art-branch-guard.yml",
            ".githooks/pre-push",
            "assets/creatures/wyvern_art_other/model.glb",
            "assets/creatures/wyvern.json",
        ):
            with self.subTest(path=path):
                self.git("switch", "-q", "--detach", "main")
                self.write(path, "changed\n")
                self.commit()
                self.guard(expected=path)

    def test_rename_from_protected_path_is_rejected(self):
        target = "art_pipeline/reports/wyvern.md"
        (self.repo / target).parent.mkdir()
        self.git("mv", "AGENTS.md", target)
        self.commit()
        self.guard(expected="AGENTS.md")

    def test_nested_repository_rule_files_are_rejected_case_insensitively(self):
        for filename in (".gitattributes", ".GITIGNORE", ".gitmodules", ".lfsconfig", "agents.md"):
            with self.subTest(filename=filename):
                self.git("switch", "-q", "--detach", "main")
                path = f"assets/creatures/wyvern_art/textures/{filename}"
                self.write(path, "untrusted repository configuration\n")
                self.commit()
                self.guard(expected=path)

    def test_rename_into_protected_path_is_rejected(self):
        target = "assets/creatures/wyvern_art_glb"
        self.git("mv", ASSET, target)
        self.commit()
        self.guard(expected=target)

    def test_art_path_cannot_be_a_symlink(self):
        blob = self.git("hash-object", "-w", "--stdin", input="../../wyvern.json")
        path = "assets/creatures/wyvern_art/link"
        self.git("update-index", "--add", "--cacheinfo", f"120000,{blob},{path}")
        self.commit(stage=False)
        self.guard(expected=path)

    def test_art_path_cannot_be_a_gitlink(self):
        head = self.git("rev-parse", "HEAD")
        path = "assets/creatures/wyvern_art/submodule"
        self.git("update-index", "--add", "--cacheinfo", f"160000,{head},{path}")
        self.commit(stage=False)
        self.guard(expected=path)

    def test_branch_id_cannot_contain_a_directory_separator(self):
        self.guard(branch="art/wyvern/secondary", expected="art/<id>")

    def test_another_order_cannot_be_changed(self):
        self.set_status("IN_PROGRESS", index=1)
        self.guard(expected="raptor")

    def test_order_definition_cannot_change(self):
        queue = copy.deepcopy(self.original)
        queue["orders"][0]["priority"] = True
        self.write_queue(queue)
        self.commit()
        self.guard(expected="priority")

    def test_duplicate_queue_keys_are_rejected(self):
        queue = json.dumps(self.original).replace(
            '"schema_version": 1', '"schema_version": 2, "schema_version": 1'
        )
        self.write(QUEUE, queue)
        self.commit()
        self.guard(expected="schema_version")

    def test_queue_accepts_windows_utf8_bom(self):
        queue = copy.deepcopy(self.original)
        queue["orders"][0]["status"] = "IN_PROGRESS"
        (self.repo / QUEUE).write_text(json.dumps(queue), encoding="utf-8-sig")
        self.commit()
        self.guard()

    def test_status_cannot_skip_in_progress(self):
        self.set_status("READY_FOR_INTEGRATION")
        self.guard(expected="TODO")

    def test_status_cannot_move_backward(self):
        self.set_status("IN_PROGRESS")
        self.set_status("TODO")
        self.guard(expected="IN_PROGRESS")

    def test_pc2_cannot_integrate_an_order(self):
        self.set_status("IN_PROGRESS")
        self.set_status("READY_FOR_INTEGRATION")
        self.set_status("INTEGRATED")
        self.guard(expected="INTEGRATED")

    def test_forbidden_intermediate_commit_cannot_be_hidden(self):
        self.write("AGENTS.md", "forbidden change\n")
        self.commit()
        self.write("AGENTS.md", "original content\n")
        self.commit()
        self.guard(expected="AGENTS.md")

    def test_new_order_appears_from_the_trusted_base(self):
        self.git("switch", "-q", "main")
        queue = copy.deepcopy(self.original)
        queue["orders"].append(order("triceratops"))
        self.write_queue(queue)
        self.commit()
        self.git("switch", "-q", "-c", "art/triceratops")
        self.set_status("IN_PROGRESS", index=2)
        self.guard(branch="art/triceratops")

    def test_stale_branch_can_receive_new_orders_from_trusted_base(self):
        self.set_status("IN_PROGRESS")
        self.git("switch", "-q", "main")
        queue = copy.deepcopy(self.original)
        queue["orders"].append(order("triceratops"))
        self.write_queue(queue)
        self.write("AGENTS.md", "trusted update\n")
        self.commit()
        self.git("switch", "-q", "art/wyvern")
        self.guard()
        self.git("merge", "--no-commit", "--no-ff", "main")
        queue["orders"][0]["status"] = "IN_PROGRESS"
        self.write_queue(queue)
        self.commit()
        self.set_status("READY_FOR_INTEGRATION")
        self.guard()

    def test_merge_cannot_restore_a_protected_file_from_before_base(self):
        self.set_status("IN_PROGRESS")
        self.git("switch", "-q", "main")
        self.write("AGENTS.md", "trusted update\n")
        self.commit()
        self.git("switch", "-q", "art/wyvern")
        self.git("merge", "--no-commit", "--no-ff", "main")
        self.write("AGENTS.md", "original content\n")
        self.commit()
        self.guard(expected="AGENTS.md")


if __name__ == "__main__":
    unittest.main()
