#pragma once

struct MySubsystem
{
    void (__stdcall *PrintHelloNodos)();
    int (__stdcall *Add)(int a, int b);
};