```bash
  sudo apt update
```
```bash
  sudo apt install build-essential libncurses-dev bison flex libssl-dev libelf-dev
```
```bash
  wget https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.17.tar.xz
```
```bash
  tar -xvf linux-6.17.tar.xz
```
```bash
  sudo apt-get install libncurses6 libncurses5-dev
```
```bash
  cd linux-6.17/
```
```bash
  make defconfig
```
```bash
  make clean
```
```bash
  git config --global user.name "username"
```
```bash
  git config --global user.email "email"
```
```bash
  git init
```
```bash
  git add .
```
```bash
  git commit -m "Initial commit for deb-pkg"
```
```bash
  git tag -a v6.14 -m "Fake tag for deb-pkg"
```
```bash
  make -j$(nproc) deb-pkg
```
```bash
  ls ../*.deb
```
```bash
  sudo dpkg -i linux*.deb
```
```bash
  sudo update-grub
```
```bash
  sudo reboot
```