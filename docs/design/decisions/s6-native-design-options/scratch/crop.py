import sys
from PIL import Image
src, dst, x, y, w, h = sys.argv[1:7]
x, y, w, h = int(x), int(y), int(w), int(h)
Image.open(src).crop((x, y, x + w, y + h)).save(dst)
