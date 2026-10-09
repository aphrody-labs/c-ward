# Installed by aphrody-libc-preload: run dynamically linked musl programs on
# the aphrody-libc overlay. Static binaries are unaffected.
case ":${LD_PRELOAD}:" in
*:/usr/lib/libaphrody_libc.so.0:*) ;;
*) export LD_PRELOAD="/usr/lib/libaphrody_libc.so.0${LD_PRELOAD:+:$LD_PRELOAD}" ;;
esac
