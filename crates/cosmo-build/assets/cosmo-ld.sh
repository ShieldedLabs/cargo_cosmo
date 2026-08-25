#!/bin/sh
# Link a rustc-produced object set into a cosmopolitan ELF for one architecture.
#
# rustc drives this as its `linker` with gnu-cc flavor, so it hands us a GCC
# command line aimed at a normal musl toolchain. Two things have to happen:
# strip the args that fight cosmo's model, and inject cosmo's CRT, linker
# script and libc in the right order.
#
# Invoked as cosmo-ld-x86_64 / cosmo-ld-aarch64; the suffix selects the arch.

set -e

ARCH=${0##*/cosmo-ld-}
COSMOCC=@COSMOCC@
BIN="$COSMOCC/bin"
LIB="$COSMOCC/$ARCH-linux-cosmo/lib"
CC="$BIN/$ARCH-linux-cosmo-gcc"

[ -x "$CC" ] || { echo "cosmo-ld: no compiler at $CC" >&2; exit 1; }

# Mirrors LDFLAGS/LDFLAGS_$ARCH in toolchain/cosmocc/bin/cosmocc. Kept in the
# same order so a diff against that script stays readable when cosmo updates.
COMMON="-static -nostdlib -no-pie -fuse-ld=bfd -Wl,-z,noexecstack -Wl,-z,norelro -Wl,--gc-sections -L$LIB"
case "$ARCH" in
   x86_64)
      CRT="$LIB/ape.o $LIB/crt.o"
      ARCHFLAGS="-Wl,-T,$LIB/ape.lds -Wl,-z,common-page-size=4096 -Wl,-z,max-page-size=16384"
      ;;
   aarch64)
      # No ape.o here: the aarch64 half is a plain ELF that apelink embeds.
      CRT="$LIB/crt.o"
      ARCHFLAGS="-Wl,-T,$LIB/aarch64.lds -Wl,-z,common-page-size=16384 -Wl,-z,max-page-size=16384"
      ;;
   *)
      echo "cosmo-ld: unsupported arch '$ARCH'" >&2; exit 1 ;;
esac

OUTPUT=
ARGS=

while [ $# -gt 0 ]; do
   arg=$1
   case "$arg" in
      # Capture the output path; we re-emit it ourselves so the CRT objects can
      # be placed immediately after it, ahead of every user object.
      -o) OUTPUT=$2; shift ;;
      -o*) OUTPUT=${arg#-o} ;;

      # cosmo is non-PIE by construction.
      -pie|-Wl,-pie|-Wl,--pic-executable) ;;

      # cosmocc links -z norelro; rustc's musl default asks for the opposite.
      # rustc emits these both separately and pre-joined as one -Wl,a,b,c arg.
      -Wl,-z,relro*|-Wl,-z,now*|-Wl,-z,defs) ;;

      # -lcosmo supplies libc, libm, pthreads, dl and the compiler runtime.
      # Letting any of these through pulls in host musl and duplicates symbols.
      -lc|-lm|-ldl|-lrt|-lutil|-lpthread|-lgcc|-lgcc_s|-lunwind) ;;

      # We force bfd and static linkage below.
      -fuse-ld=*|-Wl,-Bdynamic|-shared|-rdynamic) ;;

      # Already implied by -nostdlib.
      -nodefaultlibs|-nostartfiles|-nostdlib) ;;

      *) ARGS="$ARGS $arg" ;;
   esac
   shift
done

[ -n "$OUTPUT" ] || { echo "cosmo-ld: no -o in link line" >&2; exit 1; }

# Stage-2 normalization: route every libc call std makes through
# cosmo-compat's __wrap_* translators. Generated from the same wrap list
# the crate is built from, so the two cannot drift.
WRAPS="-Wl,--wrap=accept4 -Wl,--wrap=bind -Wl,--wrap=chdir -Wl,--wrap=chmod -Wl,--wrap=chown -Wl,--wrap=chroot -Wl,--wrap=clock_gettime -Wl,--wrap=clock_nanosleep -Wl,--wrap=close -Wl,--wrap=closedir -Wl,--wrap=connect -Wl,--wrap=copy_file_range -Wl,--wrap=dirfd -Wl,--wrap=dup -Wl,--wrap=dup2 -Wl,--wrap=execvp -Wl,--wrap=fchmod -Wl,--wrap=fchmodat -Wl,--wrap=fchown -Wl,--wrap=fcntl -Wl,--wrap=fdatasync -Wl,--wrap=fdopendir -Wl,--wrap=flock -Wl,--wrap=fork -Wl,--wrap=freeaddrinfo -Wl,--wrap=fstat -Wl,--wrap=fsync -Wl,--wrap=ftruncate -Wl,--wrap=futimens -Wl,--wrap=getaddrinfo -Wl,--wrap=getcwd -Wl,--wrap=gethostname -Wl,--wrap=getpeername -Wl,--wrap=getpwuid_r -Wl,--wrap=getrandom -Wl,--wrap=getsockname -Wl,--wrap=getsockopt -Wl,--wrap=ioctl -Wl,--wrap=kill -Wl,--wrap=killpg -Wl,--wrap=lchown -Wl,--wrap=linkat -Wl,--wrap=listen -Wl,--wrap=lseek -Wl,--wrap=lstat -Wl,--wrap=mkdir -Wl,--wrap=mkfifo -Wl,--wrap=mmap -Wl,--wrap=mprotect -Wl,--wrap=munmap -Wl,--wrap=open -Wl,--wrap=openat -Wl,--wrap=opendir -Wl,--wrap=pause -Wl,--wrap=pipe2 -Wl,--wrap=poll -Wl,--wrap=posix_spawn_file_actions_addchdir_np -Wl,--wrap=posix_spawn_file_actions_adddup2 -Wl,--wrap=posix_spawn_file_actions_destroy -Wl,--wrap=posix_spawn_file_actions_init -Wl,--wrap=posix_spawnattr_destroy -Wl,--wrap=posix_spawnattr_init -Wl,--wrap=posix_spawnattr_setflags -Wl,--wrap=posix_spawnattr_setpgroup -Wl,--wrap=posix_spawnattr_setsigdefault -Wl,--wrap=posix_spawnp -Wl,--wrap=pread -Wl,--wrap=preadv -Wl,--wrap=pthread_attr_destroy -Wl,--wrap=pthread_attr_getguardsize -Wl,--wrap=pthread_attr_getstack -Wl,--wrap=pthread_attr_init -Wl,--wrap=pthread_attr_setstacksize -Wl,--wrap=pthread_create -Wl,--wrap=pthread_detach -Wl,--wrap=pthread_getattr_np -Wl,--wrap=pthread_join -Wl,--wrap=pthread_key_create -Wl,--wrap=pthread_key_delete -Wl,--wrap=pthread_setname_np -Wl,--wrap=pthread_setspecific -Wl,--wrap=pwrite -Wl,--wrap=pwritev -Wl,--wrap=read -Wl,--wrap=readdir -Wl,--wrap=readlink -Wl,--wrap=readv -Wl,--wrap=realpath -Wl,--wrap=recv -Wl,--wrap=recvfrom -Wl,--wrap=recvmsg -Wl,--wrap=rename -Wl,--wrap=renameat -Wl,--wrap=rmdir -Wl,--wrap=sched_getaffinity -Wl,--wrap=send -Wl,--wrap=sendfile -Wl,--wrap=sendmsg -Wl,--wrap=sendto -Wl,--wrap=setgid -Wl,--wrap=setgroups -Wl,--wrap=setpgid -Wl,--wrap=setsid -Wl,--wrap=setsockopt -Wl,--wrap=setuid -Wl,--wrap=shutdown -Wl,--wrap=sigaction -Wl,--wrap=sigaddset -Wl,--wrap=sigaltstack -Wl,--wrap=signal -Wl,--wrap=socket -Wl,--wrap=socketpair -Wl,--wrap=splice -Wl,--wrap=stat -Wl,--wrap=symlink -Wl,--wrap=syscall -Wl,--wrap=sysconf -Wl,--wrap=unlink -Wl,--wrap=unlinkat -Wl,--wrap=utimensat -Wl,--wrap=waitpid -Wl,--wrap=write -Wl,--wrap=writev"

# COSMO_LD_DEBUG=<file> records the reconciled link line. It is the only way to
# see what rustc actually handed us -- which rlibs, in which order, and whether
# fat LTO merged them into one object before the linker ever saw them.
[ -n "$COSMO_LD_DEBUG" ] && printf '%s\n' "$ARGS" >> "$COSMO_LD_DEBUG"

# shellcheck disable=SC2086  # word splitting is the point for these flag sets
"$CC" -o "$OUTPUT" $CRT $COMMON $ARCHFLAGS $WRAPS $ARGS -lcosmo

# cosmocc runs this on every linked image; it rewrites the ELF into the shape
# apelink and the APE loader expect. Skipping it produces a binary that links
# cleanly and then crashes on start.
"$BIN/fixupobj" "$OUTPUT"
