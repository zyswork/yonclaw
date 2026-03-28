// 用户模型定义

export interface User {
  id: string
  enterpriseId: string
  email: string
  name: string
  passwordHash?: string
  role: 'admin' | 'manager' | 'user'
  permissions: string[]
  status: 'active' | 'inactive' | 'suspended'
  createdAt: Date
  updatedAt: Date
  lastLogin?: Date
}

export interface CreateUserRequest {
  email: string
  name: string
  role: 'admin' | 'manager' | 'user'
  permissions?: string[]
}

export interface UpdateUserRequest {
  name?: string
  role?: 'admin' | 'manager' | 'user'
  permissions?: string[]
  status?: 'active' | 'inactive' | 'suspended'
}

export interface UserResponse {
  id: string
  enterpriseId: string
  email: string
  name: string
  role: string
  permissions: string[]
  status: string
  createdAt: string
  updatedAt: string
  lastLogin?: string
}

export interface AssignPermissionRequest {
  userId: string
  permissions: string[]
}

export interface UserStatusHistory {
  id: string
  userId: string
  oldStatus: string | null
  newStatus: 'active' | 'inactive' | 'suspended'
  reason?: string
  changedBy: string
  createdAt: Date
}

export interface ChangeUserStatusRequest {
  status: 'active' | 'inactive' | 'suspended'
  reason?: string
}

export interface UserStatusResponse {
  id: string
  status: string
  statusChangedAt: string
  statusChangedBy: string
}

