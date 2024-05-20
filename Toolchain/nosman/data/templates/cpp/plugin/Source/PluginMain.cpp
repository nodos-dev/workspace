#include <Nodos/PluginAPI.h>
#include <Nodos/PluginHelpers.hpp>
#include <Nodos/Helpers.hpp>

NOS_INIT();

extern "C"
{
NOSAPI_ATTR nosResult NOSAPI_CALL nosExportNodeFunctions(size_t* outCount, nosNodeFunctions** outFunctions)
{
    *outCount = (size_t)(0);
    if (!outFunctions)
        return NOS_RESULT_SUCCESS;
    return NOS_RESULT_SUCCESS;
}
}