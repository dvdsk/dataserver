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
6. deploy files
move splitter and server executable to new users home dir
7. set startup service
create a service for starting the splitter and one for starting the server 
(in directory /etc/systemd/system (assuming debian based systemd))

######example unit file for server:
```
[Unit]
Description=Data server
After=network-online.target
Wants=network-online.target

[Service]
WorkingDirectory=/home/dataserver/
ExecStart=/home/dataserver/server --external-port 443 --port 38973 --domain <domain> --token <token> --no-menu
User=dataserver
Group=dataserver

[Install]
WantedBy=multi-user.target
```

######example unit file for splitter:
```
[Unit]
Description=Data splitter
After=network-online.target
Wants=network-online.target

[Service]
WorkingDirectory=/home/dataserver/
ExecStart=/home/dataserver/splitter
User=dataserver
Group=dataserver

[Install]
WantedBy=multi-user.target
RequiredBy=data_server
```
___

TODO:
-systemd as non root
-systemd service for stable dataserver