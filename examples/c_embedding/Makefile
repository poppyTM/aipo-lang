CC ?= gcc
CFLAGS ?= -Wall -Wextra -O2 -I../../crates/aipo-c-abi/include
LDFLAGS ?= -L../../target/debug -laipo_c_abi -Wl,-rpath,../../target/debug -lm -lpthread -ldl

TARGET = c_embedding_demo

all: $(TARGET)

$(TARGET): main.c
	$(CC) $(CFLAGS) main.c -o $(TARGET) $(LDFLAGS)

run: $(TARGET)
	./$(TARGET)

clean:
	rm -f $(TARGET)

.PHONY: all run clean
