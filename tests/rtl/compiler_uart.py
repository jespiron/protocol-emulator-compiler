"""Black-box UART integration for compiler-produced SRAM images."""

import json
import os
from pathlib import Path

import cocotb
from cocotb.triggers import ClockCycles, RisingEdge, Timer
from cocotb_tests.common import reset_top, start_clock
from cocotb_tests.fabric import drain_rx, load_program, push_tx, select_channel, start_engine

BIT = 64
TX = [0x55, 0xA3, 0x00]
RX = [0xC6, 0x81, 0x7E]


async def monitor(dut, pin):
    samples = []
    for expected in TX:
        for _ in range(10000):
            await RisingEdge(dut.clk)
            await Timer(1, unit="ns")
            if (int(dut.uio_oe.value) >> pin) & 1 and not (int(dut.uio_out.value) >> pin) & 1:
                break
        else:
            raise AssertionError("no TX start bit")
        start = cocotb.utils.get_sim_time(unit="ns")
        await ClockCycles(dut.clk, BIT + BIT // 2)
        await Timer(1, unit="ns")
        value = 0
        for bit in range(8):
            value |= ((int(dut.uio_out.value) >> pin) & 1) << bit
            await ClockCycles(dut.clk, BIT)
            await Timer(1, unit="ns")
        assert value == expected
        assert (int(dut.uio_out.value) >> pin) & 1, "missing stop bit"
        elapsed = cocotb.utils.get_sim_time(unit="ns") - start
        assert elapsed == 9.5 * BIT * 20, elapsed
        samples.append(value)
    return samples


async def drive(dut, pin):
    await ClockCycles(dut.clk, 400)
    for byte in RX:
        for bit in [0, *((byte >> i) & 1 for i in range(8)), 1]:
            dut.uio_in.value = bit << pin
            await ClockCycles(dut.clk, BIT)
        await ClockCycles(dut.clk, BIT * 2)


@cocotb.test()
async def compiled_uart(dut):
    report = json.loads(Path(os.environ["COMPILER_REPORT"]).read_text())
    image = Path(os.environ["COMPILER_IMAGE"]).read_bytes()
    tx_pin = report["pins"]["tx"]
    duplex = "rx" in report["pins"]
    tx_name = "transmitter" if duplex else "serial"
    await start_clock(dut)
    await reset_top(dut)
    if duplex:
        dut.uio_in.value = 1 << report["pins"]["rx"]
    await load_program(dut, list(image))
    await select_channel(dut, report["shifter_fifo_channels"][tx_name])
    for byte in TX:
        await push_tx(dut, byte)
    receiver = cocotb.start_soon(monitor(dut, tx_pin))
    await start_engine(dut)
    if duplex:
        sender = cocotb.start_soon(drive(dut, report["pins"]["rx"]))
        await select_channel(dut, report["shifter_fifo_channels"]["receiver"])
        assert await drain_rx(dut, len(RX)) == RX
        await sender
    assert await receiver == TX
