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

qemu-system-riscv32 -M ot-earlgrey,no-epmp-cfg=true -display none -serial mon:stdio \
    -device loader,cpu-num=0,file=target/${APP}/dist/default/final.ihex \
    -global driver=lowrisc-ibex-riscv-cpu,property=resetvec,value=0x20000000 \
    -semihosting -semihosting-config enable=on,userspace=on $@
