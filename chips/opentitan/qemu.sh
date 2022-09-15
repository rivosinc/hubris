qemu-system-riscv32 -nographic -M opentitan -global ibex-timer.timebase-freq=1000000 -device loader,addr=0x20000000,cpu-num=0,file=final.ihex -semihosting -semihosting-config enable=on,userspace=on
