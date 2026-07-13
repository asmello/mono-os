.PHONY: build run show clean

build: exit

exit: exit.o
	ld -o exit exit.o    # turn object file into an executable (add address info, resolve symbols, etc.)

exit.o: exit.s
	as -o exit.o exit.s  # assemble the source file into an object file (turns assembly instructions into machine code, but doesn't add address info or resolve symbols yet)

run: exit
	./exit; echo $$?  	  # should print 42

show: exit
	readelf -a exit 	  # inspect what ld produced

clean:
	rm -f exit.o exit