#!/usr/bin/env python3
"""Render the desktop counterpart of OpenTrack's panel mark (no external assets)."""
from pathlib import Path
from PIL import Image, ImageDraw
root = Path(__file__).resolve().parent.parent
size, scale = 256, 4
image = Image.new('RGBA', (size * scale, size * scale))
draw = ImageDraw.Draw(image)
def box(bounds):
    return tuple(value * scale for value in bounds)
draw.rounded_rectangle(box((8, 8, 248, 248)), radius=48 * scale, fill='#191b1c')
draw.rounded_rectangle(box((51, 57, 205, 199)), radius=11 * scale, outline='#efeee8', width=9 * scale)
draw.line(box((53, 96, 203, 96)), fill='#efeee8', width=8 * scale)
draw.line(box((96, 99, 96, 197)), fill='#efeee8', width=8 * scale)
draw.ellipse(box((169, 69, 185, 85)), fill='#9ab9ad')
image.resize((size, size), Image.Resampling.LANCZOS).save(root / 'icons/icon.png')
