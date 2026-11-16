"""Original night-synth soundtrack for the Nocturn Music trailer.

96 BPM: one beat = 0.625 s, one bar = 2.5 s. The cut points in edit.py sit on bars.
  0-10 s   intro: pad swell, bells, riser into the drop
  10-47.5  groove: kick/snare/hats, bass, arp; extra drive in the montage (25-32.5)
  47.5     big hit on the logo, then pad + bells fade out by ~58.5 s
Writes a 44.1 kHz stereo 16-bit WAV.
"""
import sys
import wave

import numpy as np

SR = 44100
BPM = 96
BEAT = 60 / BPM
BAR = BEAT * 4
TOTAL = 59.0
N = int(SR * TOTAL)
rng = np.random.default_rng(7)

L = np.zeros(N)
R = np.zeros(N)


def t_(dur):
    return np.arange(int(SR * dur)) / SR


def add(sig, at, gain=1.0, pan=0.0):
    i = int(at * SR)
    if i >= N:
        return
    sig = sig[: N - i]
    l = np.cos((pan + 1) * np.pi / 4)
    r = np.sin((pan + 1) * np.pi / 4)
    L[i : i + len(sig)] += sig * gain * l * 1.414
    R[i : i + len(sig)] += sig * gain * r * 1.414


def lp(x, cutoff, order=2):
    # Butterworth-shaped filter in the frequency domain (an IIR loop in Python is slow)
    n = len(x)
    f = np.fft.rfftfreq(n * 2, 1 / SR)
    X = np.fft.rfft(x, n * 2)
    H = 1 / (1 + (f / cutoff) ** (2 * order)) ** 0.5
    return np.fft.irfft(X * H)[:n]


def hp(x, cutoff, order=2):
    n = len(x)
    f = np.fft.rfftfreq(n * 2, 1 / SR)
    X = np.fft.rfft(x, n * 2)
    H = 1 - 1 / (1 + (f / cutoff) ** (2 * order)) ** 0.5
    return np.fft.irfft(X * H)[:n]


def note(n):
    """MIDI note number -> Hz"""
    return 440 * 2 ** ((n - 69) / 12)


def saw(freq, dur, detune=(0.0,)):
    t = t_(dur)
    out = np.zeros_like(t)
    for d in detune:
        f = freq * 2 ** (d / 1200)
        ph = rng.random()
        out += 2 * ((t * f + ph) % 1) - 1
    return out / len(detune)


def env(dur, a, d, s, r):
    n = int(dur * SR)
    t = np.arange(n) / SR
    e = np.ones(n) * s
    e[t < a] = t[t < a] / max(a, 1e-4)
    m = (t >= a) & (t < a + d)
    e[m] = 1 - (1 - s) * (t[m] - a) / max(d, 1e-4)
    rel = t > dur - r
    e[rel] *= np.clip((dur - t[rel]) / max(r, 1e-4), 0, 1)
    return e


# A minor, Am - F - C - G (one chord per bar)
CHORDS = [
    [57, 60, 64, 71],  # Am(add9-ish: A C E B)
    [53, 57, 60, 67],  # Fadd9 (F A C G)
    [48, 55, 60, 64],  # C (C G C E)
    [55, 59, 62, 67],  # G (G B D G)
]
ROOTS = [45, 41, 36, 43]


def chord_at(bar):
    return CHORDS[int(bar) % 4], ROOTS[int(bar) % 4]


# ---------------- pad (whole piece) ----------------
def pad(chord, dur, bright=1200):
    sig = np.zeros(int(dur * SR))
    for n_ in chord:
        sig += saw(note(n_), dur, detune=(-9, 0, 8))
    sig = lp(sig, bright, order=2) / len(chord)
    return sig * env(dur, 0.6, 0.5, 0.85, 0.9)


for b in range(0, 23):
    at = b * BAR
    chord, _ = chord_at(b)
    if at >= 57:
        break
    bright = 700 if at < 10 else (1500 if at < 47.5 else 900)
    gain = 0.22
    if at < 10:
        gain = 0.10 + 0.14 * (at / 10)
    add(pad(chord, BAR + 0.9, bright), at, gain, pan=-0.25)
    add(pad([c + 12 for c in chord], BAR + 0.9, bright * 0.8), at, gain * 0.35, pan=0.3)


# ---------------- bells (intro + outro) ----------------
def bell(freq, dur=2.2):
    t = t_(dur)
    s = np.sin(2 * np.pi * freq * t) + 0.4 * np.sin(2 * np.pi * freq * 2.76 * t) * np.exp(-t * 6)
    return s * np.exp(-t * 2.2)


BELLS = [76, 79, 81, 83, 84, 83, 79, 76]
for k in range(16):  # 0-10 s, eighth notes on the 2nd half
    at = 5.0 + k * BEAT / 2
    if at < 10:
        add(bell(note(BELLS[k % 8])), at, 0.06, pan=0.6 * np.sin(k))
for k in range(20):  # outro
    at = 47.5 + k * BEAT
    if at < 56:
        add(bell(note(BELLS[(k * 3) % 8] - (12 if k % 4 == 3 else 0))), at, 0.07 * (1 - k / 22), pan=0.5 * np.sin(k * 1.7))

# ---------------- riser into the drop (7.5 -> 10) ----------------
dur = 2.5
t = t_(dur)
noise = rng.standard_normal(len(t))
riser = hp(noise, 400) * (t / dur) ** 2.2
sweep = np.sin(2 * np.pi * np.cumsum(200 + 1800 * (t / dur) ** 2) / SR) * (t / dur) ** 3
add(riser * 0.18 + sweep * 0.05, 7.5, 1.0)
# reverse-swell before the finale hit
add(riser * 0.16, 45.0, 1.0)


# ---------------- drums ----------------
def kick():
    t = t_(0.5)
    f = 48 + 110 * np.exp(-t * 28)
    s = np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * 7)
    click = rng.standard_normal(len(t)) * np.exp(-t * 300) * 0.3
    return np.tanh((s + click) * 1.6)


def snare():
    t = t_(0.45)
    n_ = hp(rng.standard_normal(len(t)), 1200) * np.exp(-t * 11)
    body = np.sin(2 * np.pi * 190 * t) * np.exp(-t * 22)
    return n_ * 0.8 + body * 0.5


def hat(open_=False):
    t = t_(0.3 if open_ else 0.08)
    return hp(rng.standard_normal(len(t)), 7000) * np.exp(-t * (12 if open_ else 60))


K, S = kick(), snare()
DROP, END = 10.0, 47.5
nbeats = int((END - DROP) / BEAT)
for i in range(nbeats):
    at = DROP + i * BEAT
    montage = 25.0 <= at < 32.5
    if i % 2 == 0 or montage:
        add(K, at, 0.55)
    if i % 2 == 1:
        add(S, at, 0.32, pan=0.05)
    for h in range(4 if montage else 2):
        add(hat(open_=(h == 1 and not montage and i % 4 == 3)), at + h * BEAT / (4 if montage else 2), 0.07 if h % 2 == 0 else 0.045, pan=0.35)
# snare roll before the finale (last bar: 45 -> 47.5)
for k in range(16):
    at = 45.0 + k * BEAT / 4
    add(S, at, 0.08 + 0.22 * k / 16, pan=0.05 * np.sin(k))


# ---------------- bass ----------------
def bassnote(freq, dur):
    s = saw(freq, dur, detune=(-4, 4)) * 0.6 + np.sin(2 * np.pi * freq * t_(dur))
    return lp(s, 420) * env(dur, 0.005, 0.12, 0.6, 0.05)


for i in range(int((END - DROP) / (BEAT / 2))):
    at = DROP + i * BEAT / 2
    _, root = chord_at(at / BAR)
    oct_ = 12 if i % 4 == 3 else 0
    add(bassnote(note(root + oct_), BEAT / 2 * 0.95), at, 0.32)


# ---------------- arp (16ths, 17.5 -> 47.5) ----------------
def pluck(freq, dur=0.3):
    s = saw(freq, dur, detune=(-6, 6))
    return lp(s, 2600) * np.exp(-t_(dur) * 12)


for i in range(int((END - 17.5) / (BEAT / 4))):
    at = 17.5 + i * BEAT / 4
    chord, _ = chord_at(at / BAR)
    pat = [0, 1, 2, 3, 2, 1, 3, 2]
    n_ = chord[pat[i % 8]] + 12
    add(pluck(note(n_)), at, 0.075, pan=0.5 * np.sin(i * 0.4))

# ---------------- finale hit (47.5) ----------------
t = t_(4.0)
boom = np.sin(2 * np.pi * np.cumsum(38 + 70 * np.exp(-t * 6)) / SR) * np.exp(-t * 1.2)
crash = hp(rng.standard_normal(len(t)), 3000) * np.exp(-t * 1.8)
add(np.tanh(boom * 1.4) * 0.7 + crash * 0.12, 47.5, 1.0)
add(pad([45, 52, 57, 60, 64, 71], 9.0, 1100) * 1.4, 47.5, 0.25)
# soft boom on the title (5 s)
add(np.tanh(boom * 1.2) * 0.35, 5.0, 1.0)


# ---------------- space: simple stereo reverb ----------------
def reverb(x, seconds=2.4, seed=1):
    r = np.random.default_rng(seed)
    n_ = int(SR * seconds)
    ir = r.standard_normal(n_) * np.exp(-np.arange(n_) / SR * (6.9 / seconds))
    ir = lp(ir, 5000)
    size = 1 << int(np.ceil(np.log2(len(x) + n_)))
    y = np.fft.irfft(np.fft.rfft(x, size) * np.fft.rfft(ir, size))[: len(x)]
    return y / np.sqrt(np.sum(ir**2))


wetL, wetR = reverb(L, seed=1), reverb(R, seed=2)
L2 = L + 0.28 * wetL
R2 = R + 0.28 * wetR

# master: fade in/out, gentle saturation, normalise
tt = np.arange(N) / SR
fade = np.clip(tt / 0.8, 0, 1) * np.clip((TOTAL - 0.3 - tt) / 3.0, 0, 1)
mix = np.stack([L2, R2], 1) * fade[:, None]
mix = np.tanh(mix * 1.3) / np.tanh(1.3)
mix /= np.max(np.abs(mix)) / 0.89

out = sys.argv[1] if len(sys.argv) > 1 else "soundtrack.wav"
with wave.open(out, "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes((mix * 32767).astype("<i2").tobytes())
print("wrote", out, f"{TOTAL:.1f}s")
