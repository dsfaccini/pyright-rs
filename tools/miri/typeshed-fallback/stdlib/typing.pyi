class TypeVar:
    def __init__(self, name: str) -> None: ...

class _SpecialForm: ...

Generic: _SpecialForm
Protocol: _SpecialForm

def overload(func: object) -> object: ...

_T_co = TypeVar("_T_co")

class Iterator(Protocol[_T_co]):
    def __next__(self) -> _T_co: ...
