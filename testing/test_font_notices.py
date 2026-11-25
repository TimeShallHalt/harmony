"""Binary packages must carry notices for embedded UI fonts."""
import importlib.util
import hashlib
import json
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("release_package", Path(__file__).resolve().parents[1] / "scripts/release_package.py")
if spec is None or spec.loader is None:
    raise SystemExit("could not load scripts/release_package.py")
release_package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_package)


class FontNoticesTest(unittest.TestCase):
    def test_packages_keep_each_font_license_and_its_attribution(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fonts = root / "ui/fonts"
            fonts.mkdir(parents=True)
            notices = {"LICENSE-OFL.txt": "original font license", "LICENSE-OFL-DMSans.txt": "DMSans license", "README.md": "font provenance"}
            for name, text in notices.items():
                (fonts / name).write_text(text)
            (fonts / "DMSans-Regular.ttf").write_bytes(b"font embedded in binary")
            source = root / "src"
            source.mkdir()
            emoji_notice = source / "LICENSE-emojibase.txt"
            emoji_notice.write_text("original emojibase attribution and license")
            twemoji = root / "vendor/twemoji"
            twemoji.mkdir(parents=True)
            for name in ("LICENSE-GRAPHICS", "LICENSE", "README.md", "README.yapfast.md", "REVISION"):
                (twemoji / name).write_text("fixture Twemoji " + name)

            crate = root / "registry/group/source/fixture"
            crate.mkdir(parents=True)
            upstream = root / "packaging/licenses/fixture-LICENSE.txt"
            upstream.parent.mkdir(parents=True)
            upstream.write_text("fixture upstream copyright and license")
            (root / "packaging/dependency-licenses.json").write_text(json.dumps({
                "fixture-1": [{"path": "licenses/fixture-LICENSE.txt", "sha256": hashlib.sha256(upstream.read_bytes()).hexdigest(), "url": "https://example.invalid/fixture/LICENSE"}]
            }))

            package = root / "package"
            package.mkdir()
            skia = root / "skia"
            skia.mkdir()
            (root / "LICENSE.md").write_text("libwebrtc license")
            archive = root / "skia.tar"
            with tarfile.open(archive, "w"):
                pass
            with patch.object(release_package, "ROOT", root):
                deps = release_package.dependency_notices({"packages": [{"name": "fixture", "version": "1", "manifest_path": str(crate / "Cargo.toml"), "license": "MIT", "source": "fixture", "repository": "https://example.invalid/fixture"}]}, package, package / "licenses", archive, skia, root)
            copied = package / "licenses/fonts"
            self.assertEqual({p.name: p.read_text() for p in copied.iterdir()}, notices)
            self.assertEqual((package / "licenses/LICENSE-emojibase.txt").read_bytes(), emoji_notice.read_bytes())
            supplied = next(x for x in deps if x["name"] == "fixture")["license_files"]
            self.assertTrue(supplied, "workspace-only upstream license must accompany the crate")
            self.assertEqual((package / supplied[0]).read_bytes(), upstream.read_bytes())
            upstream.write_text("tampered after hashing")
            rejected = root / "rejected-package"
            rejected.mkdir()
            with patch.object(release_package, "ROOT", root), self.assertRaisesRegex(ValueError, "path/hash mismatch"):
                release_package.dependency_notices({"packages": [{"name": "fixture", "version": "1", "manifest_path": str(crate / "Cargo.toml"), "license": "MIT", "source": "fixture", "repository": "https://example.invalid/fixture"}]}, rejected, rejected / "licenses", archive, skia, root)


if __name__ == "__main__":
    unittest.main()
