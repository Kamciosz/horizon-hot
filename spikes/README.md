# Próby (spike'i)

Minimalny kod, którym sprawdzono założenia projektu. Nie jest częścią produktu i nie buduje się w workspace. Wyniki i wnioski: `docs/spike/`.

Paczki modów natywnych buduje launcher Horizon, nie `cargo`:

```bash
horizon-launcher native-build <katalog paczki> --server-output <katalog> --version <wersja> --trust-native-code
```

| Katalog | Próba | Wynik |
|---|---|---|
| `s1-hot-abi-crate/` | S1, M0.2 | `docs/spike/s1-hot-abi-w-silniku.md` |
| `s2-s5-pula-slotow/hmod/` | S2, M0.3 (`cargo horizon-mod build`) | `docs/spike/s2-pula-slotow-hmod.md` |
| `s2-s5-pula-slotow/paczka/` | S5, M0.6 (paczka zasobów serwera) | `docs/spike/s5-wyglad-slotu-z-paczki-serwera.md` |
| `s3-s4-hak-komendy-i-tick/` | S3 i S4, M0.4–M0.5 | `docs/spike/s3-…`, `docs/spike/s4-…` |
