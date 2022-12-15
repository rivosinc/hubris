{
  lib,
  runCommand,
  hubris,
  humility,
  qemu,
  port ? null,
}: let
  port_string = lib.optionalString (lib.isString port) "-p ${port}";
in
  runCommand ("hubris-qemu-tests-" + hubris.app) {}
  ''
    echo "running hubris test suite for: " ${hubris.app}

    # prepare environment
    mkdir -p $out
    export PATH=${qemu}/bin/:$PATH
    export HUMILITY_ARCHIVE=${hubris}/build-${hubris.app}.zip

    # we expect this to timeout, so ensure it always gives a good return value
    timeout -k 1s 10s ${humility}/bin/humility qemu ${port_string} | tee $out/qemu.log || true

    grep "done pass" $out/qemu.log
    echo "test suite passed"
  ''
