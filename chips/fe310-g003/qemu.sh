qemu-system-riscv32 -M sifive_u,msel=1 -nographic -serial mon:stdio -device loader,addr=0x20010000,cpu-num=0,file=final.ihex -semihosting -semihosting-config enable=on,userspace=on
