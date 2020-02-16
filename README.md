##Configure system##

From newly configured system:
1. add user (skip all options after password by pressing enter)
sudo adduser dataserver
2. login as dataserver
sudo su dataserver
3. generate fresh ssh keys with all default options
ssh-keygen
[comment]: <>  4. add the private key to github as a secret called SSH_KEY
[comment]: <>  copy its content from /home/dataserver/.ssh/id_rsa
[comment]: <>  5. add host info to git
[comment]: <>  set the github secret HOST to the ip or domain name that points to the [comment]: <>  server
4. deploy files
move server executable to new users home dir
5. set startup service
create a service for starting the server and enable it, see the example below:
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
###Optional

##dev and stable server
Using a dedicated server that duplicates new sensordata we can run a development and release server at the same time with the same data. 

1. deploy files
move datasplitter executable to /home/dataserver/
2. set startup service
create a service for starting the splitter (in directory /etc/systemd/system (assuming debian based systemd)) and enable it, see the example below:

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

##automatic certificate updating
Using certmanager a letsencrypt certificate (and corrosponding keys) can be automatically generated and kept up to date. After an update it will restart all listed systemd services that depend on the keys to be up to date.

1. add user (skip all options after password by pressing enter)
sudo adduser certmanager
2. login as certmanager
sudo su certmanager
3. deploy files
move cert_updater executable to /home/certmanager/
4. set startup service
create a service for starting the updater (in directory /etc/systemd/system (assuming debian based systemd)) and enable it, see the example below:

######example unit file for splitter:
```
[Unit]
Description=Cert Manager
After=network-online.target
Wants=network-online.target

[Service]
WorkingDirectory=/home/certmanager/
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