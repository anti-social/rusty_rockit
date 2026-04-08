#include <errno.h>

unsigned long getauxval(unsigned long type __attribute__((unused))) {
    errno = ENOENT;
    return 0;
}
