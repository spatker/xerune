#!/usr/bin/env python3
"""Makes resources/fonts/XeruneHangul-{Regular,Bold}.otf, the examples'
Korean fallback font: Source Han Sans KR cut down to Hangul (all 11,172
syllables, the jamo and compatibility jamo) and CJK punctuation.

Usage: tools/subset-hangul.py SOURCE_HAN_SANS_KR_DIR
(e.g. /usr/share/fonts/adobe-source-han-sans). Needs fontTools.

The license (SIL OFL 1.1) reserves the name "Source", and a subset is a
modified version, so the fonts are renamed "Xerune Hangul".
"""

import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

UNICODES = [
    *range(0xAC00, 0xD7A4),  # Hangul syllables
    *range(0x1100, 0x1200),  # Hangul jamo
    *range(0x3130, 0x3190),  # Hangul compatibility jamo (the keyboard's keys)
    *range(0x3000, 0x3040),  # CJK symbols and punctuation
    *range(0xFF01, 0xFF61),  # full-width forms
]
FAMILY = "Xerune Hangul"
OUT = Path(__file__).resolve().parent.parent / "resources" / "fonts"


def make(src: Path, weight: str) -> None:
    font = TTFont(src)
    options = subset.Options()
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=UNICODES)
    subsetter.subset(font)

    postscript = f"XeruneHangul-{weight}"
    names = {1: FAMILY, 2: weight, 3: f"{postscript};subset of {src.stem}", 4: f"{FAMILY} {weight}",
             6: postscript, 16: FAMILY, 17: weight}
    table = font["name"]
    for record in list(table.names):
        if record.nameID in names:
            record.string = names[record.nameID]
    # The CFF table names the font too.
    cff = font["CFF "].cff
    cff.fontNames = [postscript]
    top = cff.topDictIndex[0]
    top.FullName = f"{FAMILY} {weight}"
    top.FamilyName = FAMILY
    if hasattr(top, "FDArray"):
        for fd in top.FDArray:
            if hasattr(fd, "FontName"):
                fd.FontName = f"{postscript}-{fd.FontName.split('-')[-1]}"

    out = OUT / f"{postscript}.otf"
    font.save(out)
    print(f"{out}: {out.stat().st_size} bytes")


def main() -> None:
    src_dir = Path(sys.argv[1])
    for weight in ("Regular", "Bold"):
        make(src_dir / f"SourceHanSansKR-{weight}.otf", weight)


if __name__ == "__main__":
    main()
