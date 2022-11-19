// Code generated by protoc-gen-go-grpc. DO NOT EDIT.
// versions:
// - protoc-gen-go-grpc v1.2.0
// - protoc             (unknown)
// source: tvix/store/protos/rpc_pathinfo.proto

package storev1

import (
	context "context"
	grpc "google.golang.org/grpc"
	codes "google.golang.org/grpc/codes"
	status "google.golang.org/grpc/status"
)

// This is a compile-time assertion to ensure that this generated file
// is compatible with the grpc package it is being compiled against.
// Requires gRPC-Go v1.32.0 or later.
const _ = grpc.SupportPackageIsVersion7

// PathInfoServiceClient is the client API for PathInfoService service.
//
// For semantics around ctx use and closing/ending streaming RPCs, please refer to https://pkg.go.dev/google.golang.org/grpc/?tab=doc#ClientConn.NewStream.
type PathInfoServiceClient interface {
	// Get retrieves a PathInfo object, by using the lookup parameters in
	// GetPathInfoRequest.
	// If the PathInfo object contains a DirectoryNode, it needs to be looked
	// up separately via the DirectoryService, which is purely
	// content-addressed.
	Get(ctx context.Context, in *GetPathInfoRequest, opts ...grpc.CallOption) (*PathInfo, error)
	// Put uploads a PathInfo object to the remote end. It MUST not return
	// until the PathInfo object has been written on the the remote end.
	// The remote end MAY check if a potential DirectoryNode has already been
	// uploaded.
	// Uploading clients SHOULD obviously not steer other machines to try to
	// substitute before from the remote end before having finished uploading
	// PathInfo, Directories and Blobs.
	// The returned PathInfo object MAY contain additional narinfo signatures,
	// but is otherwise left untouched.
	Put(ctx context.Context, in *PathInfo, opts ...grpc.CallOption) (*PathInfo, error)
}

type pathInfoServiceClient struct {
	cc grpc.ClientConnInterface
}

func NewPathInfoServiceClient(cc grpc.ClientConnInterface) PathInfoServiceClient {
	return &pathInfoServiceClient{cc}
}

func (c *pathInfoServiceClient) Get(ctx context.Context, in *GetPathInfoRequest, opts ...grpc.CallOption) (*PathInfo, error) {
	out := new(PathInfo)
	err := c.cc.Invoke(ctx, "/tvix.store.v1.PathInfoService/Get", in, out, opts...)
	if err != nil {
		return nil, err
	}
	return out, nil
}

func (c *pathInfoServiceClient) Put(ctx context.Context, in *PathInfo, opts ...grpc.CallOption) (*PathInfo, error) {
	out := new(PathInfo)
	err := c.cc.Invoke(ctx, "/tvix.store.v1.PathInfoService/Put", in, out, opts...)
	if err != nil {
		return nil, err
	}
	return out, nil
}

// PathInfoServiceServer is the server API for PathInfoService service.
// All implementations must embed UnimplementedPathInfoServiceServer
// for forward compatibility
type PathInfoServiceServer interface {
	// Get retrieves a PathInfo object, by using the lookup parameters in
	// GetPathInfoRequest.
	// If the PathInfo object contains a DirectoryNode, it needs to be looked
	// up separately via the DirectoryService, which is purely
	// content-addressed.
	Get(context.Context, *GetPathInfoRequest) (*PathInfo, error)
	// Put uploads a PathInfo object to the remote end. It MUST not return
	// until the PathInfo object has been written on the the remote end.
	// The remote end MAY check if a potential DirectoryNode has already been
	// uploaded.
	// Uploading clients SHOULD obviously not steer other machines to try to
	// substitute before from the remote end before having finished uploading
	// PathInfo, Directories and Blobs.
	// The returned PathInfo object MAY contain additional narinfo signatures,
	// but is otherwise left untouched.
	Put(context.Context, *PathInfo) (*PathInfo, error)
	mustEmbedUnimplementedPathInfoServiceServer()
}

// UnimplementedPathInfoServiceServer must be embedded to have forward compatible implementations.
type UnimplementedPathInfoServiceServer struct {
}

func (UnimplementedPathInfoServiceServer) Get(context.Context, *GetPathInfoRequest) (*PathInfo, error) {
	return nil, status.Errorf(codes.Unimplemented, "method Get not implemented")
}
func (UnimplementedPathInfoServiceServer) Put(context.Context, *PathInfo) (*PathInfo, error) {
	return nil, status.Errorf(codes.Unimplemented, "method Put not implemented")
}
func (UnimplementedPathInfoServiceServer) mustEmbedUnimplementedPathInfoServiceServer() {}

// UnsafePathInfoServiceServer may be embedded to opt out of forward compatibility for this service.
// Use of this interface is not recommended, as added methods to PathInfoServiceServer will
// result in compilation errors.
type UnsafePathInfoServiceServer interface {
	mustEmbedUnimplementedPathInfoServiceServer()
}

func RegisterPathInfoServiceServer(s grpc.ServiceRegistrar, srv PathInfoServiceServer) {
	s.RegisterService(&PathInfoService_ServiceDesc, srv)
}

func _PathInfoService_Get_Handler(srv interface{}, ctx context.Context, dec func(interface{}) error, interceptor grpc.UnaryServerInterceptor) (interface{}, error) {
	in := new(GetPathInfoRequest)
	if err := dec(in); err != nil {
		return nil, err
	}
	if interceptor == nil {
		return srv.(PathInfoServiceServer).Get(ctx, in)
	}
	info := &grpc.UnaryServerInfo{
		Server:     srv,
		FullMethod: "/tvix.store.v1.PathInfoService/Get",
	}
	handler := func(ctx context.Context, req interface{}) (interface{}, error) {
		return srv.(PathInfoServiceServer).Get(ctx, req.(*GetPathInfoRequest))
	}
	return interceptor(ctx, in, info, handler)
}

func _PathInfoService_Put_Handler(srv interface{}, ctx context.Context, dec func(interface{}) error, interceptor grpc.UnaryServerInterceptor) (interface{}, error) {
	in := new(PathInfo)
	if err := dec(in); err != nil {
		return nil, err
	}
	if interceptor == nil {
		return srv.(PathInfoServiceServer).Put(ctx, in)
	}
	info := &grpc.UnaryServerInfo{
		Server:     srv,
		FullMethod: "/tvix.store.v1.PathInfoService/Put",
	}
	handler := func(ctx context.Context, req interface{}) (interface{}, error) {
		return srv.(PathInfoServiceServer).Put(ctx, req.(*PathInfo))
	}
	return interceptor(ctx, in, info, handler)
}

// PathInfoService_ServiceDesc is the grpc.ServiceDesc for PathInfoService service.
// It's only intended for direct use with grpc.RegisterService,
// and not to be introspected or modified (even as a copy)
var PathInfoService_ServiceDesc = grpc.ServiceDesc{
	ServiceName: "tvix.store.v1.PathInfoService",
	HandlerType: (*PathInfoServiceServer)(nil),
	Methods: []grpc.MethodDesc{
		{
			MethodName: "Get",
			Handler:    _PathInfoService_Get_Handler,
		},
		{
			MethodName: "Put",
			Handler:    _PathInfoService_Put_Handler,
		},
	},
	Streams:  []grpc.StreamDesc{},
	Metadata: "tvix/store/protos/rpc_pathinfo.proto",
}