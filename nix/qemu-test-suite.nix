{
  runCommand,
  hubris,
  humility,
  qemu,
  app,
}:
runCommand "hubris-qemu-tests" {}
''
    echo "running hubris test suite for: " ${app}

    # prepare environment
    mkdir -p $out
    export PATH=${qemu}/bin/:$PATH
    export HUMILITY_ARCHIVE=${hubris}/${app}/dist/default/build-${app}.zip

    ${humility}/bin/humility qemu
  # &> $out/qemu.log &

    # in reality this happend < 1 sec, but giving some buffer room
    sleep 10
    kill $(jobs -p)

    grep "done pass" $out/qemu.log

    cat $out/qemu.log

    echo "test suite passed"
''
