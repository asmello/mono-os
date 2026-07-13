.PHONY: build run show clean

build: hello

hello: hello.o
	ld hello.o -o hello    # turn object file into an executable (add address info, resolve symbols, etc.)

hello.o: hello.asm
	nasm -f elf64 hello.asm -o hello.o

run: hello
	./hello                # should print "Hello, world!"

show: hello
	readelf -a hello       # inspect what ld produced

clean:
	rm -f hello.o hello hello0
