from typing import Tuple

TX_BASE_COST = 21000
TX_DATA_COST_PER_NON_ZERO = 16
TX_DATA_COST_PER_ZERO = 4
GAS_INIT_CODE_WORD_COST = 2
TX_CREATE_COST = 32000
TX_ACCESS_LIST_ADDRESS_COST = 2400
TX_ACCESS_LIST_STORAGE_KEY_COST = 1900

def calculate_intrinsic_cost(data: bytes, to: bytes, access_list:  Tuple[Tuple[int, Tuple[bytes, ...]], ...]) -> int:
    data_cost = 0

    for byte in data:
        if byte == 0:
            data_cost += TX_DATA_COST_PER_ZERO
        else:
            data_cost += TX_DATA_COST_PER_NON_ZERO

    if to == b"":
        create_cost = TX_CREATE_COST + int(init_code_cost(len(data)))
    else:
        create_cost = 0

    access_list_cost = 0

    if access_list != None:
        for _, keys in access_list:
            access_list_cost += TX_ACCESS_LIST_ADDRESS_COST
            access_list_cost += len(keys) * TX_ACCESS_LIST_STORAGE_KEY_COST

    return TX_BASE_COST + data_cost + create_cost + access_list_cost


def init_code_cost(init_code_length: int) -> int:
    return GAS_INIT_CODE_WORD_COST * ceil32(init_code_length) // 32

def ceil32(value: int) -> int:
    ceiling = 32
    remainder = value % ceiling
    if remainder == 0:
        return value
    else:
        return value + ceiling - remainder

print(calculate_intrinsic_cost(b"", b"abc", None))