from typing import Generic, TypeVar

from helper import total

T = TypeVar("T")


class Box(Generic[T]):
    def __init__(self, item: T) -> None:
        self.item = item

    def get(self) -> T:
        return self.item


def describe(x: int | None, names: list[str]) -> str:
    if x is None:
        return "none"
    for name in names:
        try:
            x = x + len(name)
        except ValueError:
            break
    box = Box(x)
    print(f"{box.get()!r:>10}")  # a comment
    return str(box.get()) + undefined_name
