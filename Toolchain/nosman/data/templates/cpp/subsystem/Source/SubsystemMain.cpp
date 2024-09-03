// Copyright Nodos AS. All Rights Reserved.
#include <mySubsystem/PublicHeader.h>
#include <Nodos/SubsystemAPI.h>

NOS_INIT()
NOS_BEGIN_IMPORT_DEPS()
NOS_END_IMPORT_DEPS()

template <typename T>
T __stdcall Add(T a, T b)
{
	return a + b;
}

void __stdcall PrintHelloNodos()
{
	nosEngine.LogI("Hello Nodos!");
}

static std::unordered_map<uint32_t, MySubsystem*> GExported;

nosResult OnRequest(uint32_t minor, void** outSubsystemCtx)
{
    auto it = GExported.find(minor);
    if (it == GExported.end())
    {
        switch (minor)
        {
        case 0:
        {
            MySubsystem* subsystem = new MySubsystem();
            subsystem->PrintHelloNodos = PrintHelloNodos;
            subsystem->Add = Add<int>;
            GExported[minor] = subsystem;
            *outSubsystemCtx = subsystem;
            return NOS_RESULT_SUCCESS;
        }
        }
        return NOS_RESULT_NOT_FOUND;
    }
    *outSubsystemCtx = it->second;
    return NOS_RESULT_SUCCESS;
}

nosResult NOSAPI_CALL OnPreUnloadSubsystem()
{
    for (auto& pair : GExported)
        delete pair.second;
    return NOS_RESULT_SUCCESS;
}

extern "C"
{
NOSAPI_ATTR nosResult NOSAPI_CALL nosExportSubsystem(nosSubsystemFunctions* subsystemFunctions)
{
    subsystemFunctions->OnRequest = OnRequest;
    subsystemFunctions->OnPreUnloadSubsystem = OnPreUnloadSubsystem;
    return NOS_RESULT_SUCCESS;
}
}