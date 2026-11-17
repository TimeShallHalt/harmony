#!/usr/bin/env python3
"""Render Harmony's original SVG at native Windows icon sizes.

Reproduce with: uv run --with resvg-py==0.5.0 --with pillow python scripts/generate-harmony-art.py
The renderer and Pillow are build-time tools, not application dependencies.
"""
from io import BytesIO
from pathlib import Path
import resvg_py
from PIL import Image, ImageDraw, IcoImagePlugin

ROOT = Path(__file__).resolve().parents[1]
SIZES = (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)


def main():
    source = ROOT / "packaging/icon.svg"
    images = [Image.open(BytesIO(resvg_py.svg_to_bytes(svg_path=str(source), width=n, height=n, skip_system_fonts=True))).convert("RGBA") for n in SIZES]
    destination = ROOT / "packaging/windows/harmony.ico"
    destination.parent.mkdir(parents=True, exist_ok=True)
    images[-1].save(destination, format="ICO", sizes=[(n, n) for n in SIZES], append_images=images[:-1])
    # The large icon also serves shell integrations and brand documentation.
    png = resvg_py.svg_to_bytes(svg_path=str(source), width=512, height=512, skip_system_fonts=True)
    (ROOT / "packaging/harmony.png").write_bytes(png)
    loaded = Image.open(destination)
    assert isinstance(loaded, IcoImagePlugin.IcoImageFile)
    actual = loaded.ico.sizes()
    expected = {(n, n) for n in SIZES}
    assert actual == expected, (actual, expected)
    for n in SIZES:
        frame = loaded.ico.getimage((n, n)).convert("RGBA")
        assert frame.size == (n, n) and frame.getbbox()
    print(f"Rendered {destination}: {len(actual)} verified native sizes")
    sheet = Image.new("RGB", (640, 240), "#101422")
    draw = ImageDraw.Draw(sheet)
    x = 20
    for image in images[:7]:
        sheet.paste(image, (x, 38), image)
        draw.text((x, 112), str(image.width), fill="#dfe5ff")
        x += image.width + 18
    sheet.paste(images[-1].resize((156, 156)), (464, 34), images[-1].resize((156, 156)))
    draw.text((20, 176), "HARMONY / two voices, one connection", fill="#c7bdff")
    evidence = ROOT / ".local/evidence/upstream-041-branding"
    evidence.mkdir(parents=True, exist_ok=True)
    sheet.save(evidence / "brand-icons.png")


if __name__ == "__main__":
    main()
