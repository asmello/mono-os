.PHONY: build run show clean

build: exit

exit: exit.o
	ld exit.o -o exit        # turn object file into an executable (add address info, resolve symbols, etc.)

exit.o: exit.asm
	nasm -f elf64 exit.asm -o exit.o

run: exit
	./exit; echo $$?          # should print 42

show: exit
	readelf -a exit          # inspect what ld produced

clean:
	rm -f exit.o exit