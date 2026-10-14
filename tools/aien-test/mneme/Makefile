CC ?= cc
CFLAGS ?= -O2 -Wall -Wextra -Werror -fPIC

libmneme.so: mneme_preload.c
	$(CC) $(CFLAGS) -shared -o $@ $< -ldl

clean:
	rm -f libmneme.so

.PHONY: clean
