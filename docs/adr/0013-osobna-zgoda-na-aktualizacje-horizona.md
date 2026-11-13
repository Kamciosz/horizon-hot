# ADR 0013 — samodzielne aktualizacje Horizona wymagają osobnej zgody

- **Data:** 2026-11-13
- **Status:** accepted
- **Autor:** Szymon Sosnowski

## Kontekst

Silnik z `hot-host` to kod natywny. Horizon wymaga, żeby budowę takiego kodu potwierdził właściciel (`--trust-native-code`). Instalator i kontener pytają o tę zgodę raz, przy pierwszej budowie, a właściciel czyta wtedy kod `crates/hot-host` w konkretnej wersji.

`hotd horizon` (M9.1, D18) buduje silnik sam, za każdym razem, gdy pojawi się nowe Nightly. Nowe Nightly to nowy kod Horizona, a mixiny `hot-host` wchodzą w niego przed kompilacją. Właściciel, który zgodził się na jedną budowę przy instalacji, nie zgodził się jeszcze na kolejne budowy bez swojego udziału.

## Rozważane opcje

1. **Zgoda z instalacji wystarcza:** `--trust-native-code` włącza też samodzielne aktualizacje.
2. **Osobna zgoda:** samodzielne budowanie nowych wersji włącza dopiero druga, jawna opcja. Bez niej `hotd horizon` sprawdza nowe Nightly, odrzuca te bez `libloading` i w `hot status` pisze, że czeka na zgodę, ale niczego nie buduje.
3. **Brak samodzielnych aktualizacji:** właściciel uruchamia aktualizację ręcznie.

## Decyzja

Wybieramy **opcję 2**:
- instalator dodaje `--trust-native-code` do usługi `horizon-hot-horizon` tylko przy `--trust-native-code --auto-update-horizon`;
- kontener robi to samo tylko przy `HOT_TRUST_NATIVE_CODE=1` i `HOT_AUTO_UPDATE_HORIZON=1` (w `compose.yaml` obie zmienne są zakomentowane);
- bez zgody usługa i tak działa: wykrywa nowe wersje i mówi w `hot status`, na co czeka.

## Konsekwencje

- Domyślna instalacja nie buduje nowego kodu bez wiedzy właściciela.
- Bez drugiej opcji serwer zostaje na Nightly z instalacji, a właściciel widzi w `hot status`, że jest nowsze.
- Agent AI nigdy nie włącza tej opcji sam (zasada z `AGENTS.md` o kodzie natywnym).
