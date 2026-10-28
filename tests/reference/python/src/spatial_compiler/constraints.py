"""Finite resource assignment. Feasible allocations are witnesses, not estimates."""

from collections.abc import Mapping, Sequence

from .model import CompileError


def allocate_pins(candidates: Mapping[str, Sequence[int]], capacity: int = 8) -> dict[str, int]:
    if any(
        not pads or any(type(p) is not int or not 0 <= p < capacity for p in pads)
        for pads in candidates.values()
    ):
        raise CompileError("pin-pad", f"each pin needs candidates within 0..{capacity - 1}")
    owner: dict[int, str] = {}

    def augment(name: str, seen: set[int]) -> bool:
        for pad in sorted(set(candidates[name])):
            if pad in seen:
                continue
            seen.add(pad)
            previous = owner.get(pad)
            if previous is None or augment(previous, seen):
                owner[pad] = name
                return True
        return False

    for name in sorted(candidates):
        if not augment(name, set()):
            detail = ", ".join(f"{n}:{list(p)}" for n, p in sorted(candidates.items()))
            raise CompileError(
                "pin-capacity", f"cannot assign distinct pads to {detail}; available={capacity}"
            )
    return {name: pad for pad, name in sorted(owner.items())}


def allocate_units(names: Sequence[str], capacity: int, kind: str) -> dict[str, int]:
    if len(set(names)) != len(names):
        raise CompileError("name", f"duplicate {kind} names")
    if len(names) > capacity:
        raise CompileError(
            "tile-capacity", f"{kind}: requested {len(names)}, available {capacity}: {list(names)}"
        )
    return dict(zip(names, range(len(names)), strict=True))
