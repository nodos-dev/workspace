// Copyright Nodos AS. All Rights Reserved.
#include <mySubsystem/PublicHeader.h>
#include <Nodos/PluginAPI.h>

NOS_INIT()
NOS_BEGIN_IMPORT_DEPS()
NOS_END_IMPORT_DEPS()

int AddInt(int a, int b)
{
	return a + b;
}

void PrintHelloNodos()
{
	nosEngine.LogI("Hello Nodos!");
}

static std::unordered_map<uint32_t, MyPlugin*> GExported;

nosResult ExportAPI(uint32_t minor, void** outSubsystemCtx)
{
    auto it = GExported.find(minor);
    if (it == GExported.end())
    {
        switch (minor)
        {
        case 0:
        {
            MyPlugin* subsystem = new MyPlugin();
            subsystem->PrintHelloNodos = PrintHelloNodos;
            subsystem->Add = AddInt;
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
NOSAPI_ATTR nosResult NOSAPI_CALL nosExportPlugin(nosPluginFunctions* subsystemFunctions)
{
    subsystemFunctions->OnRequestAPI = ExportAPI;
    subsystemFunctions->OnPreUnloadPlugin = OnPreUnloadSubsystem;
    return NOS_RESULT_SUCCESS;
}
}