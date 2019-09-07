#!/bin/bash
cross build --target=armv7-unknown-linux-gnueabihf --example simple
scp target/armv7-unknown-linux-gnueabihf/debug/examples/simple pi@192.168.1.10:/home/pi/dataserver/dataserver
