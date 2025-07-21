#pragma once

struct MyPlugin
{
    void (*PrintHelloNodos)();
    int (*Add)(int a, int b);
};