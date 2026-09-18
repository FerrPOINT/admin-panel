import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "verify_readme.py"


def load_validator():
    spec = importlib.util.spec_from_file_location("verify_readme", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class VerifyReadmeTests(unittest.TestCase):
    def make_repo(self, readme: str, files: dict[str, bytes | str] | None = None) -> Path:
        root = Path(tempfile.mkdtemp())
        (root / "README.md").write_text(readme, encoding="utf-8")
        for relative, content in (files or {}).items():
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if isinstance(content, bytes):
                target.write_bytes(content)
            else:
                target.write_text(content, encoding="utf-8")
        return root

    def test_accepts_valid_local_link_image_and_header_anchor(self) -> None:
        validator = load_validator()
        root = self.make_repo(
            '<a href="#features"><img src="badge.svg" alt="Features" /></a>\n'
            '<a name="features"></a>\n'
            '![Architecture](docs/architecture.svg)\n'
            '[Architecture notes](docs/notes.md)\n',
            {
                "badge.svg": "<svg/>",
                "docs/architecture.svg": "<svg/>",
                "docs/notes.md": "# Notes\n",
            },
        )

        self.assertEqual([], validator.validate(root))

    def test_reports_missing_local_link(self) -> None:
        validator = load_validator()
        root = self.make_repo("[Missing](docs/missing.md)\n")

        self.assertIn("RMD002", "\n".join(validator.validate(root)))

    def test_reports_missing_local_image(self) -> None:
        validator = load_validator()
        root = self.make_repo("![Missing](docs/missing.svg)\n")

        self.assertIn("RMD003", "\n".join(validator.validate(root)))

    def test_reports_missing_header_anchor(self) -> None:
        validator = load_validator()
        root = self.make_repo('<a href="#quality"><img src="badge.svg" alt="Quality" /></a>\n', {"badge.svg": "<svg/>"})

        self.assertIn("RMD004", "\n".join(validator.validate(root)))

    def test_reports_placeholder_and_absolute_local_path(self) -> None:
        validator = load_validator()
        root = self.make_repo("Run {{PORT}} from /opt/dev/example.\n")

        findings = "\n".join(validator.validate(root))
        self.assertIn("RMD005", findings)
        self.assertIn("RMD006", findings)

    def test_reports_ci_badge_without_declared_workflow(self) -> None:
        validator = load_validator()
        root = self.make_repo(
            "![CI](https://github.com/FerrPOINT/example/actions/workflows/ci.yml/badge.svg)\n"
        )

        self.assertIn("RMD009", "\n".join(validator.validate(root)))


if __name__ == "__main__":
    unittest.main()
