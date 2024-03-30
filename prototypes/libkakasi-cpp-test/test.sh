#!/bin/bash

./build.sh 
locale 
build/kakasi_cpp_test_exec 

if [ $(uname) == "Linux" ]; then
#    kakasi -JH -f -i utf-8 -o utf-8 /usr/share/kakasi/itaijidict /usr/share/kakas#i/kanwadict <<< "最近人気の デスクトップな リナックスです!"
    kakasi -JH -f -i utf-8 -o utf-8 ../../assets/itaijidict ../../assets/kanwadict <<< "最近人気の デスクトップな リナックスです!"
else
    kakasi -JH -f -i utf-8 -o utf-8 ..\\..\\assets\\itaijidict ..\\..\\assets\\kanwadict <<< "最近人気の デスクトップな リナックスです!"
fi

