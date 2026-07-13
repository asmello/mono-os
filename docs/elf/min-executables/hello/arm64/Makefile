
.PHONY: build run show clean

build: hello

hello: hello.o
	ld -o hello hello.o    # turn object file into an executable (add address info, resolve symbols, etc.)

hello.o: hello.s
	as -o hello.o hello.s  # assemble the source file into an object file (turns assembly instructions into machine code, but doesn't add address info or resolve symbols yet)

run: hello
	./hello                # should print "Hello, world!"

show: hello
	readelf -a hello 	   # inspect what ld produced

clean:
	rm -f hello.o hello