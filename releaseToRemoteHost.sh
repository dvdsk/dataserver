cross build --target=armv7-unknown-linux-gnueabihf --release
scp target/armv7-unknown-linux-gnueabihf/debug/dataserver pi@192.168.1.10:/home/pi/dataserver/dataserver
