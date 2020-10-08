## Configure system

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
(in directory `/etc/systemd/system` (assuming debian based systemd)). Note the keys argument in the example is the path that should be used it certmanager is configured which is optional (see below).

###### example unit file for server:
```
[Unit]
Description=Data server
After=network-online.target
Wants=network-online.target

[Service]
WorkingDirectory=/home/dataserver/
ExecStart=/home/dataserver/server \
        --external-port 443 \
        --port 38973 \
        --domain <domain> \
        --token <token> \
        --keys /home/certmanager/keys
        --no-menu
ExecStop=/bin/kill -s SIGKILL $MAINPID
User=dataserver
Group=dataserver

[Install]
WantedBy=multi-user.target
```
### Optional

## dev and stable server
Using a dedicated server that duplicates new sensordata we can run a development and release server at the same time with the same data. 

1. deploy files
move datasplitter executable to `/home/dataserver/`
2. set startup service
create a service for starting the splitter (in directory `/etc/systemd/system` (assuming debian based systemd)) and enable it, see the example below:

###### example unit file for splitter:
```
[Unit]
Description=Data splitter
After=network-online.target
Wants=network-online.target

[Service]
WorkingDirectory=/home/dataserver/
ExecStart=/home/dataserver/splitter \
        --domain "deviousd.duckdns.org" \
        --keys "/home/certmanager/keys"
ExecStop=/bin/kill -s SIGKILL $MAINPID
User=dataserver
Group=dataserver

[Install]
WantedBy=multi-user.target
RequiredBy=data_server
```

## automatic certificate updating
Using certmanager a letsencrypt certificate (and corrosponding keys) can be automatically generated and kept up to date. After an update it will restart all listed systemd services that depend on the keys to be up to date.

1. add user (skip all options after password by pressing enter)
sudo adduser certmanager
2. login as certmanager
sudo su certmanager
3. deploy files
move cert_updater executable to /home/certmanager/
4. run updater configure file
after the inital run (with arguments) a file config.yaml will appear, add your domain and a comma seperated of "-enclosed systemd services that should be restarted on certificate update.
5. change/add key dir arguments
change the keys dir of services that depend on updated keys to `/home/certmanager/keys`
6. setup keys for group access
create a group called cert
`sudo groupadd cert` 
change the group of the keys dir to cert
`sudo chown certmanager:cert keys` 
allow read access for the cert group
`sudo chmod 750 keys` 
set the setgid bit on the keys dir so files in keys get the right permissions
`sudo chmod g+s keys`
7. add servers to group cert
you can add other users to this goup by replacing dataserver with theire username:
`sudo usermod -a -G cert dataserver`
8. [Required if running without sudo (recommanded)]
Allow the certmanager to restart the required unit files.
add a file `/etc/sudoers.d/certmanager` with a line:
```
%certmanager ALL= NOPASSWD: /bin/systemctl restart dataserver.service
```
do this for all services whos keys are managed by updater replacing dataserver with theire service name. For example add datasplitter.
8. set startup service
create a service for starting the updater (in directory `/etc/systemd/system` (assuming debian based systemd)) and enable it, see the example below:


###### example unit file for cert updater:
```
[Unit]
Description=Cert Manager
After=network-online.target
Wants=network-online.target

[Service]
WorkingDirectory=/home/certmanager/
ExecStart=/home/certmanager/updater --log warn --port 38313
ExecStop=/bin/kill -s SIGKILL $MAINPID
User=certmanager
Group=certmanager

[Install]
WantedBy=multi-user.target
RequiredBy=data_server
```
___

TODO:
-systemd as non root
-systemd service for stable dataserver