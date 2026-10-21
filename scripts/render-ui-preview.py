"""Render exported Ratatui cells for visual inspection without a music session.

PowerShell:
  $env:TUITIFY_UI_PREVIEW = "$PWD/target/ui-preview.json"
  cargo test --locked redesigned_layout_preserves_queue_mouse_rows_and_seek
  cargo test --locked ui_preview_covers_the_main_views
  python scripts/render-ui-preview.py target/ui-preview.json target/ui-preview
"""

import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def color(value, fallback):
    if value.startswith("#"):
        return value
    return fallback


def render(screens, destination):
    destination.mkdir(parents=True, exist_ok=True)
    fonts = Path("C:/Windows/Fonts")
    normal = ImageFont.truetype(str(fonts / "consola.ttf"), 19)
    bold = ImageFont.truetype(str(fonts / "consolab.ttf"), 19)
    symbols = ImageFont.truetype(str(fonts / "seguisym.ttf"), 19)
    cell_width, cell_height, inset = 11, 25, 22
    symbol_font = ("⌕", "♡", "▶", "Ⅱ", "▥", "▪", "■", "►", "▌", "▸")
    # Block elements are drawn as real geometry: a terminal font fills the
    # cell, the preview font does not.
    blocks = {
        "█": (0.0, 1.0),
        "▀": (0.0, 0.5),
        "▄": (0.5, 1.0),
        "▁": (0.875, 1.0),
        "▂": (0.75, 1.0),
        "▃": (0.625, 1.0),
        "▅": (0.375, 1.0),
        "▆": (0.25, 1.0),
        "▇": (0.125, 1.0),
    }
    for screen in screens:
        width, height = screen["width"], screen["height"]
        image = Image.new("RGB", (width * cell_width + inset * 2, height * cell_height + inset * 2), "#0a0f0f")
        draw = ImageDraw.Draw(image)
        for index, cell in enumerate(screen["cells"]):
            x = inset + (index % width) * cell_width
            y = inset + (index // width) * cell_height
            draw.rectangle((x, y, x + cell_width - 1, y + cell_height - 1), fill=color(cell["bg"], "#0a0f0f"))
            symbol = cell["symbol"]
            fill = color(cell["fg"], "#e9f1ee")
            if symbol in blocks:
                top, bottom = blocks[symbol]
                draw.rectangle(
                    (x, y + int(cell_height * top), x + cell_width - 1, y + int(cell_height * bottom)),
                    fill=fill,
                )
                continue
            font = symbols if symbol in symbol_font else bold if cell["bold"] else normal
            draw.text((x, y + 1), symbol, fill=fill, font=font)
        path = destination / f"{screen.get('name', 'queue')}-{width}x{height}.png"
        image.save(path)
        print(path.resolve())


if __name__ == "__main__":
    render(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8")), Path(sys.argv[2]))
