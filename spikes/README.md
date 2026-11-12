# Próby (spike'i)

Minimalny kod, którym sprawdzono założenia projektu. Nie jest częścią produktu i nie buduje się w workspace. Wyniki i wnioski: `docs/spike/`.

Paczki modów natywnych buduje launcher Horizon, nie `cargo`:

```bash
horizon-launcher native-build <katalog paczki> --server-output <katalog> --version <wersja> --trust-native-code
```
