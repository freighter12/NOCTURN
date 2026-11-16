# Nocturn Music trailer (cinematic cut)

1. Record the footage: run the **iPhone trailer (simulator recording)** workflow
   (`.github/workflows/ios-trailer.yml`) and download the `iphone-trailer` artifact.
2. Make the soundtrack (original, generated, no samples):
   `python music.py soundtrack.wav`
3. Cut the trailer (Windows: uses Segoe UI from C:/Windows/Fonts; needs ffmpeg, numpy, Pillow):
   `python edit.py --src iphone-trailer.mov --t0 <s> --music soundtrack.wav --out nocturn-trailer.mp4`

`--t0` is when the app's timeline starts in the recording. The simulator drifts a
little from Trailer.swift's schedule, so each shot's `src` (seconds after t0) in
`SHOTS` is set by looking at the footage. `--stills 12,20` renders single frames
and `--from/--to` renders a part, for quick checks.
