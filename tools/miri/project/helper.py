from main import Box, describe


def total(boxes: list[Box[int]]) -> int:
    n = 0
    for b in boxes:
        while n < b.get():
            n = n + 1
    del boxes
    return n + describe(n, [])
