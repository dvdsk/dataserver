##Deploy dev server##
Copy the binary to some location on the hosts machine and run it

##Deploy stable server##
Should happen automatically on push on stable branch if the server is configured as below. 

##Configure system##
From newly configured system:
1. add user (skip all options after password by pressing enter)
sudo adduser dataserver
2. login as dataserver
sudo su dataserver
3. generate fresh ssh keys with all default options
ssh-keygen
4. add the private key to github as a secret called SSH_KEY
copy its content from /home/dataserver/.ssh/id_rsa
5. add host info to git
set the github secret HOST to the ip or domain name that points to the server

TODO:
-systemd as non root
-systemd service for stable dataserver