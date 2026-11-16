gcc -I cadkit-capi/include quickstart.c -L cadkit-capi/lib -lcadkit_capi -o quickstart
LD_LIBRARY_PATH=cadkit-capi/lib ./quickstart     # or link with an rpath
