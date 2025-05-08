import os
from PIL import Image

OUTPUT_FILE = 'portraits.png'

def crop_center(img, w, h, eye_y_frac=1/3):
    iw, ih = img.size
    scale = max(w/iw, h/ih)
    nw, nh = int(iw*scale), int(ih*scale)
    img = img.resize((nw, nh), Image.LANCZOS)
    # Center eyes at top third
    eye_y = int(nh * eye_y_frac)
    top = max(0, eye_y - h//3)
    left = (nw - w)//2
    box = (left, top, left+w, top+h)
    return img.crop(box)

def main():
    files = sorted([f for f in os.listdir('.') if f.lower().startswith('leader') and f.lower().endswith(('.png','.jpg','.jpeg'))])
    portraits = []
    w, h = 600, 900
    processed_count = 0

    for f in files:
        processed_count += 1
        print(f'Processing {processed_count}/{len(files)}: {f}')
        img = Image.open(f)
        cropped = crop_center(img, w, h)
        portraits.append(cropped)
    if not portraits: return
    sheet = Image.new('RGBA', (w*len(portraits), h))
    for i, p in enumerate(portraits):
        sheet.paste(p, (i*w, 0))
    sheet.save(OUTPUT_FILE)
    print(f'Generating {OUTPUT_FILE} complete!')

if __name__ == '__main__':
    main()