# Contents
==========
* [Description](#description)
* [Building](#building)
* [Setting Up Users](#setting_up_users)
* [Configuration](#configuration)
* [Sytem Info](#sytem_info)
* [Running](#running)

## Description
Toe is a [Finger](https://datatracker.ietf.org/doc/html/rfc742) protocol server
written in Rust which aims to be simple and (hopefully) more secure than
historical implementations of the protocol. Toe shares less information with
it's default settings, runs in a chroot and drops root priviledges as soon as
possible during it's startup procedure. The increased security comes at the
expense of a moderately more involved setup.

## Building
Toe requires no system dependencies besides libc and is built using Cargo.
```Sh
git clone https://codeberg.org/jeang3nie/toe
cd toe
cargo build --release
```
## Setting Up Users
In it's simplest setup one would set the server root to /home and the chroot
option to false. Any user wishing to serve their .plan file would also have to
have the permissions of their home directory set to world readable, as unlike
fingerd Toe will not run as the root user.

Toe wants to be run in a chroot. For increased security it is recommended that
you create a dedicated directory for toe to run in, with a directory and .plan
file for each user who will be sharing their plan using the server. The .plan
file can then be symlinked into the user's home directory for easy editing.
```Sh
# Users jack and jill want to share their .plan using toe
# First, create those directories
sudo install -dv /srv/toe/{jack,jill}
# Now create their .plan files
sudo touch /srv/toe/jack/.plan
sudo touch /srv/toe/jill/.plan
# Now we'll change ownership of those files so the users can edit them
sudo chown -R jack:jack /srv/toe/jack
sudo chown -R jill:jill /srv/toe/jill
# Finally, we'll symlink them into the user's home directory so that the user
# can just edit $HOME/.plan to change their plan
ln -s /srv/toe/jack/.plan /home/jack
ln -s /srv/toe/jill/.plan /home/jill
```
It is also neccessary to set up a user and group to run the server as. Feel free
to use a different user and group here, but make sure that the user and group
that you create matches what you put in `toe.toml` - see
[Configuration](#configuration).
```Sh
# Most Linux distros
groupadd -r toe
useradd -r -g toe toe
```
> Note the `-r` flag in the above commands - this creates a `system` user rather
> than a regular user. The user which the server runs as should not have a login
> shell, home directory, or own any files.
## Configuration
Configuration is in [Toml](https://toml.io/en/) format. Toe wants to find it's
config file at /etc/toe.toml. An example `toe.toml` file is included in the
`data` directory of the source distribution. It is recommended in particular
that the `address` field be changed from it's default value of "0.0.0.0", which
binds on all interfaces, to whatever the machine's public IP is.
## System Info
Traditionally, when no user is requested, fingerd would give out various system
information such as uptime, users and processor stats. The internet was a less
dangerous place back then than it is now and it is up to the user to decide if
serving any of this information is appropriate for your use case. The various
types of information which Toe is capable of serving up can be turned on and off
via settings in `toe.toml`.

If Toe is to be run in a chroot, more work must be done to make most of this
information available, as it is gathered from the kernel virtual filesystems
mounted at /proc and /sys. If desired, then those virtual filesystems can be
mounted inside the chroot. Additionally, information about the number of users
requires access to`/etc/passwd`. It is possible to either bind mount the actual
`/etc/passwd` file or, preferably, to just crate a dummy version with only the
users desired to be counted in the listing, leaving out all system user accounts.
```Sh
# Create the directories proc and sys inside of the server root
sudo install -dv /srv/toe/{proc,sys}
mount the virtual filesystems
sudo mount -t proc proc /srv/toe/proc
sudo mount -t sysfs sysfs /srv/toe/sys
# Create /srv/toe/etc
install -dv /srv/toe/etc
# Naive method - bind mounting /etc/passwd
touch /srv/toe/etc/passwd
mount -Bv /etc/passwd /srv/toe/etc/passwd
# Better - filter users into a dummy file
grep -v "bin/nologin" /etc/passwd > /srv/toe/etc/passwd
```
The virtual kernel systems could be made to be autmatically mounted inside Toe's
root directory by placing appropriate lines in `/etc/fstab`.
## Running
Toe is started by invoking `toe` on the commandline. It must be started by the
root user, after which it will drop priviledges and run as the user and group
which are configured in `toe.toml`. If logging is desired, any startup script
should direct the program's stdout and stderr to the appropriate logs.
