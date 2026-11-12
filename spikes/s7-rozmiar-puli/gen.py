#!/usr/bin/env python3
"""
PL: Generuje źródło `hot_slots` (S7) z N blokami-slotami i M przedmiotami-slotami.
    Pliki jak w S2 (`spikes/s2-s5-pula-slotow/hmod/`), bez models/textures/lang (D8).
EN: Generates the `hot_slots` source (S7) with N slot blocks and M slot items.
    Same files as S2, without models/textures/lang (D8).

Użycie / usage: gen.py <katalog wyjściowy> <bloki> <przedmioty>
Potem / then:   cd <katalog> && cargo horizon-mod build
"""

import json
import sys
from pathlib import Path


def write(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, separators=(",", ":")))


def main() -> None:
    out, blocks, items = Path(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
    out.mkdir(parents=True, exist_ok=False)
    (out / "horizon-mod.toml").write_text(
        'id = "hot_slots"\n'
        'name = "Hot Slots (spike S7)"\n'
        f'version = "0.7.{blocks}"\n'
        f'description = "Spike S7 horizon-hot: {blocks} slot blocks, {items} slot items."\n'
        'authors = ["Szymon Sosnowski"]\n'
        'api = "0.2"\n'
        'side = "both"\n'
        'capabilities = ["registries"]\n'
    )
    a, d = out / "assets/hot_slots", out / "data/hot_slots"
    for i in range(blocks):
        name = f"slot_{i}"
        model = f"hot_slots:block/{name}"
        write(d / f"horizon/block/{name}.json", {"template": "minecraft:stone", "properties": {"hardness": 1.5}})
        write(d / f"loot_table/blocks/{name}.json", {
            "type": "minecraft:block",
            "pools": [{"rolls": 1, "entries": [{"type": "minecraft:item", "name": f"hot_slots:{name}"}]}],
        })
        write(a / f"blockstates/{name}.json", {"variants": {"": {"model": model}}})
        write(a / f"items/{name}.json", {"model": {"type": "minecraft:model", "model": model}})
    for i in range(items):
        name = f"item_{i}"
        write(d / f"horizon/item/{name}.json", {"template": "minecraft:stick"})
        write(a / f"items/{name}.json", {"model": {"type": "minecraft:model", "model": f"hot_slots:item/{name}"}})


if __name__ == "__main__":
    main()
