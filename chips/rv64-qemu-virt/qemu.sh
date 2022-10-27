qemu-system-riscv64 -M virt -nographic -serial mon:stdio  -device loader,addr=0x90000400,cpu-num=0,file=final.bin -semihosting -semihosting-config enable=on,userspace=on -m 8G -bios none
