#pragma once

struct MySubsystem
{
    void (*PrintHelloNodos)();
    int (*Add)(int a, int b);
};