#!/bin/bash
if [ ! -x "$(command -v qemu-system-riscv32)" ]; then
  echo 'Error: qemu-system-riscv32 is the PATH.' >&2
  exit 1
fi

export APP=$1
shift

if [ -z "${APP}" ]; then
  echo "usage: qemu.sh APP"
  exit 1
fi

if [ ! -d target/${APP} ]; then
  echo "Error Directory ./target/" ${APP} " did not exist"
  exit 1
fi

qemu-system-riscv32 -M opentitan -display none -serial mon:stdio \
    -kernel target/${APP}/dist/default/final.elf \
    -global driver=riscv.lowrisc.ibex.soc,property=resetvec,value=0x20000000 \
    -semihosting -semihosting-config enable=on,userspace=on $@

