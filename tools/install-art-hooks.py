"""Enable the repository's hooks without silently deactivating unrelated local hooks."""
import subprocess
import sys
from pathlib import Path


def git(*args):
    return subprocess.check_output(["git", *args], text=True, encoding="utf8").strip()


def main():
    root = Path(git("rev-parse", "--show-toplevel"))
    target = root / ".githooks"
    installed = Path(git("rev-parse", "--path-format=absolute", "--git-path", "hooks"))
    shipped = {"post-checkout", "post-commit", "post-merge", "pre-push"}
    if not all((target / name).is_file() for name in shipped):
        raise ValueError("Les quatre hooks versionnés doivent être présents.")
    if installed.resolve() != target.resolve():
        # Compare the entire hook, including its instructions, with this Git LFS version.
        manual = git("-c", "core.hooksPath=.git/hooks", "lfs", "update", "--manual")
        examples = {paragraph.strip().replace("\n\t", "\n") for paragraph in manual.split("\n\n")
                    if paragraph.lstrip().startswith("#!/")}
        for path in installed.glob("*"):
            if path.name.endswith(".sample"):
                continue
            if path.name not in shipped or not path.is_file() or path.is_symlink():
                raise ValueError(f"Hook personnalisé à examiner avant de changer de chemin : {path}")
            if path.read_text(encoding="utf8").strip() not in examples:
                raise ValueError(f"Hook différent de celui fourni par Git LFS : {path}")
            print(f"Hook LFS existant conservé sur disque : {path}")
    git("lfs", "install", "--local", "--skip-repo")
    git("config", "--local", "core.hooksPath", ".githooks")
    print("Hooks versionnés actifs pour ce clone ; Git LFS configuré.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        sys.exit(str(exc))
