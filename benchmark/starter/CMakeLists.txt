cmake_minimum_required(VERSION 3.16)
project(CPhysKernel VERSION 0.1.0 LANGUAGES C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_C_EXTENSIONS OFF)

option(BUILD_TESTS "Build tests" ON)
option(BUILD_EXAMPLES "Build examples" ON)
option(ENABLE_ASAN "Enable AddressSanitizer" OFF)
option(ENABLE_UBSAN "Enable UndefinedBehaviorSanitizer" OFF)

if(ENABLE_ASAN)
  add_compile_options(-fsanitize=address -fno-omit-frame-pointer)
  add_link_options(-fsanitize=address)
endif()
if(ENABLE_UBSAN)
  add_compile_options(-fsanitize=undefined -fno-omit-frame-pointer)
  add_link_options(-fsanitize=undefined)
endif()

add_compile_options(-Wall -Wextra -Wpedantic -Werror)

add_library(cphys INTERFACE)
target_include_directories(cphys INTERFACE include)

if(BUILD_TESTS)
  enable_testing()
  add_subdirectory(tests)
endif()
if(BUILD_EXAMPLES)
  add_subdirectory(examples)
endif()
