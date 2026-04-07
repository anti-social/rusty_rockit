// getauxval_stub.c
#include <errno.h>
#include <stdint.h>

unsigned long getauxval(unsigned long type) {
    errno = ENOENT;
    return 0;
}
