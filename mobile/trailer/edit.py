"""Cinematic trailer for Nocturn Music, cut from the simulator recording.

  python edit.py --src iphone-trailer.mov --t0 5.7 --music soundtrack.wav --out trailer.mp4
  python edit.py ... --stills 3,12,20      # a few frames as PNGs to check the look
  python edit.py ... --from 20 --to 30     # render only part (fast preview)

--t0 is the time in the recording at which the app's trailer timeline starts
(the "setup" step of Trailer.swift); every shot's source time is relative to it.
"""
import argparse
import math
import subprocess
import sys
from dataclasses import dataclass, field

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

W, H, FPS = 1920, 1080, 30
SRC_W, SRC_H = 1206, 2622
SCREEN_H = 900
SCREEN_W = round(SCREEN_H * SRC_W / SRC_H)  # 414
RADIUS = 58
BEZEL = 11
FONTS = "C:/Windows/Fonts/"
LAVENDER = (185, 166, 255)


def font(name, size):
    return ImageFont.truetype(FONTS + name, size)


def ease(x):
    x = min(max(x, 0.0), 1.0)
    return x * x * (3 - 2 * x)


def ease_out(x):
    x = min(max(x, 0.0), 1.0)
    return 1 - (1 - x) ** 3


def rounded_mask(w, h, r, ss=4):
    m = Image.new("L", (w * ss, h * ss), 0)
    ImageDraw.Draw(m).rounded_rectangle((0, 0, w * ss - 1, h * ss - 1), r * ss, fill=255)
    return m.resize((w, h), Image.LANCZOS)


# ---------------------------------------------------------------- shots
@dataclass
class Shot:
    start: float
    end: float
    kind: str = "app"  # app | intro | finale
    src: float = 0.0  # seconds after t0
    scale: float = 1.0
    cx: int = 1300
    focus: tuple = (0.5, 0.5)  # screen y-fraction at canvas centre, start -> end
    zoom: tuple = (1.0, 1.035)
    fade_in: float = 0.35
    rise: bool = False
    punch: bool = False  # quick zoom-settle on a hard cut
    layer: dict = field(default_factory=dict)


SHOTS = [
    Shot(0, 10, "intro", fade_in=0),
    Shot(10, 15, src=3.3, fade_in=0.9, rise=True, zoom=(1.0, 1.04)),  # Home
    Shot(15, 17.5, src=8.0, scale=2.1, cx=1370, focus=(0.11, 0.12), zoom=(1.0, 1.02)),  # Continue on iPhone
    Shot(17.5, 22.5, src=11.0, scale=1.7, cx=1280, focus=(0.34, 0.42), zoom=(1.0, 1.03)),  # lyrics close-up
    Shot(22.5, 25, src=17.2, zoom=(1.0, 1.04)),  # galaxy
    # montage, hard cuts on the half bar
    Shot(25.0, 26.25, src=20.2, scale=1.35, focus=(0.42, 0.46), fade_in=0, punch=True),  # synthwave
    Shot(26.25, 27.5, src=23.2, fade_in=0, punch=True),  # rain
    Shot(27.5, 28.75, src=26.2, scale=1.35, focus=(0.48, 0.52), fade_in=0, punch=True),  # city
    Shot(28.75, 30.0, src=29.1, fade_in=0, punch=True),  # seasons
    Shot(30.0, 31.25, src=31.7, scale=1.35, focus=(0.40, 0.44), fade_in=0, punch=True),  # christmas
    Shot(31.25, 32.5, src=34.1, fade_in=0, punch=True),  # meteors
    Shot(32.5, 37.5, src=36.6),  # lyric card
    Shot(37.5, 42.5, src=42.9),  # Wrapped
    Shot(42.5, 47.5, src=51.7),  # themes
    Shot(47.5, 58.5, "finale", fade_in=0),
]
TOTAL = 58.5

CAPTIONS = [
    (10.5, 14.6, "HOME", "Everything you love,\nin one place."),
    (15.15, 17.35, "FROM YOUR PC", "Pick up right\nwhere you left off."),
    (17.8, 22.25, "LYRICS", "Every word,\nright on time."),
    (22.8, 32.25, "BACKGROUNDS", "A sky for\nevery song."),
    (32.8, 37.25, "LYRIC CARDS", "Share the line\nthat hits."),
    (37.8, 42.25, "WRAPPED", "See what kept you\nup at night."),
    (42.8, 47.2, "THEMES", "Make it\nyours."),
]


# ---------------------------------------------------------------- source video
class Source:
    def __init__(self, path, t0, stills):
        self.path, self.t0, self.stills = path, t0, stills
        self.pipes = {}
        self.last = {}

    def _size(self, shot):
        return round(SCREEN_W * shot.scale), round(SCREEN_H * shot.scale)

    def frame(self, si, shot, local_t):
        w, h = self._size(shot)
        if self.stills:
            ss = self.t0 + shot.src + local_t
            cmd = ["ffmpeg", "-v", "error", "-ss", f"{ss:.3f}", "-i", self.path, "-frames:v", "1",
                   "-vf", f"scale={w}:{h}:flags=lanczos", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]
            raw = subprocess.run(cmd, capture_output=True).stdout
            return Image.frombytes("RGB", (w, h), raw) if len(raw) == w * h * 3 else Image.new("RGB", (w, h))
        idx = round(local_t * FPS)
        p = self.pipes.get(si)
        if p is None:
            ss = self.t0 + shot.src + local_t
            cmd = ["ffmpeg", "-v", "error", "-ss", f"{ss:.3f}", "-i", self.path,
                   "-vf", f"fps={FPS},scale={w}:{h}:flags=lanczos", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]
            p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=w * h * 3 * 2)
            self.pipes[si] = p
            self.last[si] = (idx - 1, Image.new("RGB", (w, h)))
        li, img = self.last[si]
        while li < idx:
            raw = p.stdout.read(w * h * 3)
            if len(raw) < w * h * 3:
                break  # recording ended: hold the last frame
            img = Image.frombytes("RGB", (w, h), raw)
            li += 1
        self.last[si] = (max(li, idx), img)
        return img

    def close(self, si):
        p = self.pipes.pop(si, None)
        if p:
            p.kill()


# ---------------------------------------------------------------- phone
def build_phone(shot):
    s = shot.scale
    sw, sh = round(SCREEN_W * s), round(SCREEN_H * s)
    bz = round(BEZEL * s)
    r = round(RADIUS * s)
    pad = round(70 * s)
    fw, fh = sw + 2 * bz, sh + 2 * bz
    layer = Image.new("RGBA", (fw + 2 * pad, fh + 2 * pad), (0, 0, 0, 0))
    # soft shadow
    sh_m = Image.new("L", layer.size, 0)
    ImageDraw.Draw(sh_m).rounded_rectangle((pad, pad + round(28 * s), pad + fw, pad + fh + round(28 * s)), r + bz, fill=170)
    sh_m = sh_m.filter(ImageFilter.GaussianBlur(34 * s))
    layer.paste((0, 0, 0, 255), (0, 0), sh_m)
    # side buttons
    d = ImageDraw.Draw(layer)
    btn = (58, 58, 66, 255)
    bw = max(2, round(4 * s))
    for y0, y1 in ((0.20, 0.25), (0.29, 0.36), (0.38, 0.45)):
        d.rounded_rectangle((pad - bw, pad + fh * y0, pad + 2, pad + fh * y1), bw, fill=btn)
    d.rounded_rectangle((pad + fw - 2, pad + fh * 0.30, pad + fw + bw, pad + fh * 0.42), bw, fill=btn)
    # titanium frame: a light rim, then the dark bezel
    rim = rounded_mask(fw, fh, r + bz)
    layer.paste((96, 96, 106, 255), (pad, pad), rim)
    inner = rounded_mask(fw - 2 * max(2, round(2.5 * s)), fh - 2 * max(2, round(2.5 * s)), r + bz - 2)
    o = max(2, round(2.5 * s))
    layer.paste((14, 14, 17, 255), (pad + o, pad + o), inner)
    shot.layer = {
        "frame": layer,
        "pad": pad,
        "bz": bz,
        "mask": rounded_mask(sw, sh, r),
        "sw": sw,
        "sh": sh,
    }


# ---------------------------------------------------------------- static layers
def radial(w, h, inner, outer, cy=0.42, power=1.6):
    y, x = np.mgrid[0:h, 0:w]
    d = np.sqrt(((x - w / 2) / (w * 0.75)) ** 2 + ((y - h * cy) / (h * 0.9)) ** 2)
    d = np.clip(d, 0, 1) ** power
    a, b = np.array(inner, float), np.array(outer, float)
    return (a * (1 - d[..., None]) + b * d[..., None]).astype(np.uint8)


VIGNETTE = None
GRAIN = []
LEFT_SHADE = None
NIGHT = None


def init_static():
    global VIGNETTE, GRAIN, LEFT_SHADE, NIGHT
    y, x = np.mgrid[0:H, 0:W]
    d = np.sqrt(((x - W / 2) / (W / 2)) ** 2 + ((y - H / 2) / (H / 2)) ** 2)
    VIGNETTE = (1 - 0.42 * np.clip(d - 0.35, 0, 1) ** 1.5).astype(np.float32)[..., None]
    rng = np.random.default_rng(3)
    GRAIN = [rng.normal(0, 4.0, (H, W, 1)).astype(np.float32) for _ in range(6)]
    a = np.clip(1 - x[0] / 1150, 0, 1) ** 1.4 * 150
    shade = np.zeros((H, W, 4), np.uint8)
    shade[..., 3] = a[None, :].astype(np.uint8)
    LEFT_SHADE = Image.fromarray(shade, "RGBA")
    NIGHT = Image.fromarray(radial(W, H, (22, 22, 32), (4, 4, 7)))


def blurred_bg(screen):
    # a portrait frame, blurred into a landscape mood backdrop
    tiny = screen.resize((48, 104), Image.BILINEAR)
    band = tiny.crop((0, 52 - 14, 48, 52 + 13))
    bg = band.resize((192, 108), Image.BICUBIC).filter(ImageFilter.GaussianBlur(9))
    bg = bg.resize((W, H), Image.BICUBIC)
    arr = np.asarray(bg).astype(np.float32) * 0.42 + np.array([6, 5, 12], np.float32)
    return Image.fromarray(np.clip(arr, 0, 255).astype(np.uint8))


# ---------------------------------------------------------------- text
def tracked(draw, xy, text, fnt, fill, tracking, anchor_center=True):
    widths = [fnt.getlength(c) for c in text]
    total = sum(widths) + tracking * (len(text) - 1)
    x, y = xy
    if anchor_center:
        x -= total / 2
    for c, cw in zip(text, widths):
        draw.text((x, y), c, font=fnt, fill=fill)
        x += cw + tracking
    return total


def caption_layer(label, headline):
    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    lines = headline.split("\n")
    size = 74
    while True:
        f = font("segoeuil.ttf", size)
        if max(f.getlength(l) for l in lines) <= 700 or size < 40:
            break
        size -= 2
    lh = size * 1.14
    block = 46 + lh * len(lines)
    x0, y0 = 150, H / 2 - block / 2
    tracked(d, (x0, y0), label, font("seguisb.ttf", 24), LAVENDER + (255,), 6, anchor_center=False)
    d.line((x0, y0 + 36, x0 + 40, y0 + 36), fill=LAVENDER + (160,), width=2)
    for i, l in enumerate(lines):
        d.text((x0 - 3, y0 + 46 + i * lh), l, font=f, fill=(245, 245, 250, 255))
    glow = img.filter(ImageFilter.GaussianBlur(14))
    out = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    out.alpha_composite(Image.fromarray((np.asarray(glow) * np.array([1, 1, 1, 0.35])).astype(np.uint8), "RGBA"))
    out.alpha_composite(img)
    return out


CAP_LAYERS = {}


def with_alpha(img, a):
    if a >= 0.999:
        return img
    arr = np.asarray(img).copy()
    arr[..., 3] = (arr[..., 3].astype(np.float32) * a).astype(np.uint8)
    return Image.fromarray(arr, "RGBA")


# ---------------------------------------------------------------- moon + stars
def moon_image(size):
    ss = 2
    n = 1024 * ss
    yy, xx = np.mgrid[0:n, 0:n] / ss
    disk = (xx - 512) ** 2 + (yy - 512) ** 2 <= 300**2
    bite = (xx - 628) ** 2 + (yy - 430) ** 2 <= 262**2
    m = (disk & ~bite).astype(np.float32)
    # moon gradient (icon colours)
    g = np.clip(((xx - 0.2 * 1024) * 0.6 + (yy - 0.1 * 1024) * 0.85) / (1024 * 0.95), 0, 1)
    c0, c1, c2 = np.array([244, 245, 248.0]), np.array([201, 204, 212.0]), np.array([126, 130, 142.0])
    col = np.where(g[..., None] < 0.55, c0 + (c1 - c0) * (g[..., None] / 0.55), c1 + (c2 - c1) * ((g[..., None] - 0.55) / 0.45))
    star = ((xx - 742) ** 2 + (yy - 300) ** 2 <= 14**2).astype(np.float32) * 0.9
    alpha = np.clip(m + star, 0, 1)
    rgb = np.where(star[..., None] > 0, np.array([233, 235, 240.0]), col)
    img = Image.fromarray(np.dstack([rgb, alpha * 255]).astype(np.uint8), "RGBA").resize((1024, 1024), Image.LANCZOS)
    # halo
    halo = Image.new("L", (1024, 1024), 0)
    ImageDraw.Draw(halo).ellipse((512 - 330, 512 - 330, 512 + 330, 512 + 330), fill=70)
    halo = halo.filter(ImageFilter.GaussianBlur(90))
    out = Image.new("RGBA", (1024, 1024), (201, 210, 230, 0))
    out.putalpha(halo)
    out.alpha_composite(img)
    return out.resize((size, size), Image.LANCZOS)


class Sky:
    def __init__(self):
        rng = np.random.default_rng(11)
        n = 520
        self.x = rng.uniform(0, W + 200, n)
        self.y = rng.uniform(0, H, n)
        self.b = rng.uniform(0.15, 1.0, n) ** 2
        self.ph = rng.uniform(0, 6.28, n)
        self.sp = rng.uniform(0.6, 2.2, n)
        self.big = rng.random(n) > 0.94
        self.moon = moon_image(330)
        self.moon_small = moon_image(250)

    def stars(self, t, alpha=1.0):
        img = NIGHT.copy()
        d = ImageDraw.Draw(img)
        tw = 0.65 + 0.35 * np.sin(self.ph + t * self.sp)
        xs = (self.x - t * 7) % (W + 200) - 100
        for x, y, b, big, k in zip(xs, self.y, self.b, self.big, tw):
            v = int(255 * b * k * alpha)
            if v < 6:
                continue
            c = (v, v, min(255, int(v * 1.08)))
            if big:
                d.ellipse((x - 1.6, y - 1.6, x + 1.6, y + 1.6), fill=c)
            else:
                d.point((x, y), fill=c)
        return img


SKY = None


def title_block(img, t_in, tracking_em, y, alpha, music_alpha=None):
    """NOCTURN wordmark + MUSIC, drawn onto img (RGB)."""
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    f = font("segoeuil.ttf", 118)
    tracked(d, (W / 2, y), "NOCTURN", f, (240, 241, 246, 255), 118 * tracking_em)
    ma = alpha if music_alpha is None else music_alpha
    if ma > 0:
        tracked(d, (W / 2 + 14, y + 152), "MUSIC", font("seguisb.ttf", 28), LAVENDER + (int(255 * ma / max(alpha, 1e-3)) if alpha > 0 else 0,), 22)
    glow = layer.filter(ImageFilter.GaussianBlur(18))
    img.paste(Image.new("RGB", (W, H), (200, 205, 230)), (0, 0), with_alpha(glow, 0.5 * alpha).getchannel("A"))
    img.paste(layer.convert("RGB"), (0, 0), with_alpha(layer, alpha).getchannel("A"))


def render_intro(t):
    img = SKY.stars(t, alpha=ease(t / 2.0))
    if t < 5:
        # cold open: moon rises, then a line of text
        a = ease((t - 0.4) / 2.4) * (1 - ease((t - 4.4) / 0.6) * 0.0)
        y = 300 + 30 * (1 - ease_out((t - 0.4) / 3.0))
        m = with_alpha(SKY.moon, a)
        img.paste(m, (W // 2 - 165, int(y)), m)
        ta = ease((t - 1.6) / 0.9) * (1 - ease((t - 4.25) / 0.6))
        if ta > 0:
            layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
            tracked(ImageDraw.Draw(layer), (W / 2, 700), "Some songs belong to the night.", font("segoeuil.ttf", 46), (222, 225, 236, 255), 2.5)
            img.paste(layer.convert("RGB"), (0, 0), with_alpha(layer, ta).getchannel("A"))
    else:
        lt = t - 5
        y = 300 - 120 * ease((lt) / 1.6)
        m = SKY.moon
        img.paste(m, (W // 2 - 165, int(y)), m)
        a = ease(lt / 1.2)
        title_block(img, lt, 0.30 + 0.16 * ease_out(lt / 5.0), 560, a, music_alpha=ease((lt - 0.9) / 1.0))
        # flash on the downbeat
        if lt < 0.5:
            img = Image.blend(img, Image.new("RGB", (W, H), (255, 255, 255)), 0.35 * (1 - lt / 0.5))
    return img


def render_finale(t):
    lt = t - 47.5
    img = SKY.stars(t + 30)
    y = 180 - 20 * ease_out(lt / 6)
    m = SKY.moon
    img.paste(m, (W // 2 - 165, int(y)), m)
    title_block(img, lt, 0.42 + 0.05 * ease_out(lt / 8), 470, 1.0, music_alpha=ease((lt - 0.5) / 1.0))
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    a1 = ease((lt - 2.6) / 1.0)
    a2 = ease((lt - 4.4) / 1.0)
    if a1 > 0:
        tracked(d, (W / 2, 760), "Your music, after dark.", font("segoeuil.ttf", 50), (235, 236, 242, 255), 2)
        img.paste(layer.convert("RGB"), (0, 0), with_alpha(layer, a1).getchannel("A"))
    if a2 > 0:
        layer2 = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        tracked(ImageDraw.Draw(layer2), (W / 2, 860), "iPHONE   ·   PC", font("seguisb.ttf", 22), (160, 162, 176, 255), 7)
        img.paste(layer2.convert("RGB"), (0, 0), with_alpha(layer2, a2).getchannel("A"))
    if lt < 0.6:
        img = Image.blend(img, Image.new("RGB", (W, H), (255, 255, 255)), 0.55 * (1 - lt / 0.6) ** 1.5)
    # fade to black
    fb = ease((t - 56.8) / 1.6)
    if fb > 0:
        img = Image.blend(img, Image.new("RGB", (W, H)), fb)
    return img


# ---------------------------------------------------------------- compositing
def render_app(si, shot, t, src):
    lt = t - shot.start
    if not shot.layer:
        build_phone(shot)
    L = shot.layer
    screen = src.frame(si, shot, lt)
    canvas = blurred_bg(screen)
    dur = shot.end - shot.start
    p = ease(lt / dur)
    focus = shot.focus[0] + (shot.focus[1] - shot.focus[0]) * p
    top = H / 2 - focus * L["sh"]
    left = shot.cx - L["sw"] / 2
    dy = 0
    if shot.rise:
        dy = 760 * (1 - ease_out(lt / 1.4))
    fx = int(left - L["bz"] - L["pad"])
    fy = int(top - L["bz"] - L["pad"] + dy)
    canvas.paste(L["frame"], (fx, fy), L["frame"])
    canvas.paste(screen, (int(left), int(top + dy)), L["mask"])
    canvas.paste((0, 0, 0), (0, 0), LEFT_SHADE.getchannel("A"))
    z = shot.zoom[0] + (shot.zoom[1] - shot.zoom[0]) * p
    if shot.punch:
        z *= 1 + 0.05 * (1 - ease_out(lt / 0.5))
    if abs(z - 1) > 1e-4:
        cw, ch = W / z, H / z
        x0, y0 = (W - cw) / 2, (H - ch) / 2
        canvas = canvas.resize((W, H), Image.BICUBIC, box=(x0, y0, x0 + cw, y0 + ch))
    return canvas


def render_shot(si, t, src):
    shot = SHOTS[si]
    if shot.kind == "intro":
        return render_intro(t)
    if shot.kind == "finale":
        return render_finale(t)
    return render_app(si, shot, t, src)


def render(t, src):
    # the shot that owns t, plus the previous one while dissolving
    cur = max(i for i, s in enumerate(SHOTS) if s.start <= t + 1e-6)
    img = render_shot(cur, t, src)
    s = SHOTS[cur]
    if s.fade_in > 0 and t - s.start < s.fade_in and cur > 0:
        prev = render_shot(cur - 1, t, src)
        img = Image.blend(prev, img, ease((t - s.start) / s.fade_in))
    for (a, b, label, head) in CAPTIONS:
        if a - 0.01 <= t <= b:
            key = (label, head)
            if key not in CAP_LAYERS:
                CAP_LAYERS[key] = caption_layer(label, head)
            fin = ease_out((t - a) / 0.55)
            fout = 1 - ease((t - (b - 0.4)) / 0.4)
            al = min(fin, fout)
            lay = CAP_LAYERS[key]
            dy = int(22 * (1 - fin))
            if dy:
                lay = lay.transform(lay.size, Image.AFFINE, (1, 0, 0, 0, 1, -dy))
            img.paste(lay.convert("RGB"), (0, 0), with_alpha(lay, al).getchannel("A"))
    arr = np.asarray(img).astype(np.float32) * VIGNETTE
    arr += GRAIN[int(t * FPS) % len(GRAIN)]
    return np.clip(arr, 0, 255).astype(np.uint8)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--src", required=True)
    ap.add_argument("--t0", type=float, required=True)
    ap.add_argument("--music")
    ap.add_argument("--out", default="trailer.mp4")
    ap.add_argument("--stills")
    ap.add_argument("--from", dest="t_from", type=float, default=0)
    ap.add_argument("--to", dest="t_to", type=float, default=TOTAL)
    a = ap.parse_args()
    global SKY
    init_static()
    SKY = Sky()
    if a.stills:
        src = Source(a.src, a.t0, stills=True)
        for ts in a.stills.split(","):
            t = float(ts)
            Image.fromarray(render(t, src)).save(f"still_{t:05.2f}.png")
            print("still", t)
        return
    src = Source(a.src, a.t0, stills=False)
    cmd = ["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-"]
    if a.music:
        cmd += ["-ss", f"{a.t_from:.3f}", "-i", a.music, "-af", "loudnorm=I=-14:TP=-1.5:LRA=11", "-c:a", "aac", "-b:a", "256k"]
    cmd += ["-c:v", "libx264", "-preset", "slow", "-crf", "16", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
            "-t", f"{a.t_to - a.t_from:.3f}", a.out]
    enc = subprocess.Popen(cmd, stdin=subprocess.PIPE)
    n0, n1 = round(a.t_from * FPS), round(a.t_to * FPS)
    active = set()
    for n in range(n0, n1):
        t = n / FPS
        cur = max(i for i, s in enumerate(SHOTS) if s.start <= t + 1e-6)
        for i in list(active):
            if i < cur - 1:
                src.close(i)
                active.discard(i)
        active.update({cur, cur - 1})
        enc.stdin.write(render(t, src).tobytes())
        if n % 60 == 0:
            print(f"{t:5.1f}s / {a.t_to:.1f}", flush=True)
    enc.stdin.close()
    enc.wait()
    print("wrote", a.out)


if __name__ == "__main__":
    main()
